#!/usr/bin/env python3
"""Discover Ouster sensors, prompt for one, print ouster-cli source target on stdout (IP preferred)."""

from __future__ import annotations

import functools
import ipaddress
import json
import re
import select
import shutil
import socket
import subprocess
import sys
import time


class RestartDiscovery(Exception):
    """Ctrl-C during pick — caller rediscovers."""


AUTO_PICK_S = 10.0


def _color_stderr() -> bool:
    try:
        return sys.stderr.isatty()
    except (AttributeError, ValueError):
        return False


_C = (
    dict.fromkeys(("reset",), "\033[0m")
    | {
        "info": "\033[1;36m",
        "query": "\033[1;35m",
        "warn": "\033[1;33m",
        "bad": "\033[1;31m",
        "muted": "\033[2m",
        "hl": "\033[1;97m",
        "accent": "\033[94m",
    }
)


def _tag_line(kind: str, tag: str, text: str) -> None:
    if _color_stderr():
        c = _C.get(kind, _C["info"])
        print(f"{c}{tag}{_C['reset']} {text}", file=sys.stderr, flush=True)
    else:
        print(f"{tag} {text}", file=sys.stderr, flush=True)


def _prompt_pick(n: int, auto_label: str) -> None:
    tag = "[?]"
    body = (
        f"Pick [1-{n}] — auto-selecting {auto_label} in {int(AUTO_PICK_S)}s if idle; "
        "Ctrl-C: rediscover: "
    )
    if _color_stderr():
        sys.stderr.write(
            f"{_C['query']}{tag}{_C['reset']} {_C['accent']}{body}{_C['reset']}"
        )
    else:
        sys.stderr.write(f"{tag} {body}")
    sys.stderr.flush()


def parse_discover_array(text: str) -> list | None:
    """Return the first top-level JSON array in *text* (stdout may be prefixed with logs)."""
    for i, ch in enumerate(text):
        if ch != "[":
            continue
        try:
            data = json.loads(text[i:])
        except json.JSONDecodeError:
            continue
        if isinstance(data, list):
            return data
    return None


def discover_devices(ouster_cli: str) -> list | None:
    _tag_line("info", "[*]", "Discovering sensors (ouster-cli discover -o json)…")
    r = subprocess.run(
        [ouster_cli, "discover", "-o", "json"],
        capture_output=True,
        text=True,
        check=False,
    )
    merged = r.stdout
    if r.stderr.strip():
        merged = f"{r.stderr}\n{merged}"
    parsed = parse_discover_array(merged)
    if parsed is not None:
        _tag_line(
            "info",
            "[*]",
            f"Discover finished ({len(parsed)} "
            f"{'entry' if len(parsed) == 1 else 'entries'}); choose one below.",
        )
    return parsed


def _append_ipv4(cands: list[str], addr: object) -> None:
    if isinstance(addr, str):
        addr = addr.strip()
        if not addr:
            return
        if "/" in addr:
            addr = addr.partition("/")[0]
        try:
            o = ipaddress.ip_address(addr)
        except ValueError:
            return
        if o.version == 4:
            cands.append(str(o))


def _ipv4_candidates(dev: dict) -> list[str]:
    cands: list[str] = []
    net = dev.get("network")
    if isinstance(net, dict):
        v4 = net.get("ipv4")
        if isinstance(v4, dict):
            for key in ("addr", "override", "link_local"):
                _append_ipv4(cands, v4.get(key))
    addrs = dev.get("addresses")
    if isinstance(addrs, list):
        for a in addrs:
            if isinstance(a, str):
                cand = a.split("%", 1)[0].strip()
                _append_ipv4(cands, cand)
    seen: set[str] = set()
    out: list[str] = []
    for c in cands:
        if c not in seen:
            seen.add(c)
            out.append(c)
    return out


def _ipv4_rank(ip: str) -> tuple[int, str]:
    try:
        a = ipaddress.ip_address(ip)
    except ValueError:
        return (9, ip)
    if a.version != 4:
        return (9, ip)
    if ip.startswith("169.254."):
        return (3, ip)
    if a.is_private:
        return (0, ip)
    if a.is_global:
        return (1, ip)
    return (2, ip)


def connection_target(dev: dict) -> str:
    """Prefer routable IPv4 (avoids unresolved .local inside minimal nss/db). Fallbacks: api hostname, mdns hostname."""
    cands = _ipv4_candidates(dev)
    if cands:
        non_ll = [c for c in cands if not c.startswith("169.254.")]
        pool = non_ll if non_ll else cands
        return min(pool, key=_ipv4_rank)
    net = dev.get("network")
    if isinstance(net, dict):
        hn = net.get("hostname")
        if isinstance(hn, str) and hn.strip():
            return hn.strip().rstrip(".")
    hn = dev.get("hostname") or ""
    return hn.rstrip(".")


def format_line(dev: dict) -> str:
    mdns = (dev.get("hostname") or "").rstrip(".")
    si = dev.get("sensor_info") or {}
    if not isinstance(si, dict):
        si = {}
    fw = si.get("image_rev") or si.get("build_rev") or "n/a"
    prod = si.get("prod_line") or "?"
    sn = si.get("prod_sn") or "?"
    tgt = connection_target(dev)
    via = "" if tgt == mdns or not mdns else f" → source via {tgt}"
    return f"{mdns}{via} — {fw} [{prod} SN {sn}]"


def _sensor_info(dev: dict) -> dict:
    si = dev.get("sensor_info")
    return si if isinstance(si, dict) else {}


_FW_RE = re.compile(r"(\d+)\.(\d+)\.(\d+)")


def _fw_tuple(dev: dict) -> tuple[int, int, int] | None:
    si = _sensor_info(dev)
    for key in ("image_rev", "build_rev"):
        raw = si.get(key)
        if isinstance(raw, str):
            m = _FW_RE.search(raw)
            if m:
                return (int(m.group(1)), int(m.group(2)), int(m.group(3)))
    return None


def _fw_is_newer(
    best: tuple[int, int, int] | None,
    cand: tuple[int, int, int] | None,
) -> bool:
    """True if *cand* is a parsed semver strictly newer than *best* (None = unknown)."""
    if cand is None:
        return False
    if best is None:
        return True
    return cand > best


def _nested_status_running(obj: object) -> bool:
    """True if any dict in *obj* has ``status == \"RUNNING\"`` (sensor metadata)."""
    if isinstance(obj, dict):
        if obj.get("status") == "RUNNING":
            return True
        return any(_nested_status_running(v) for v in obj.values())
    if isinstance(obj, list):
        return any(_nested_status_running(v) for v in obj)
    return False


def _active_config(dev: dict) -> dict:
    cfg = dev.get("active_config")
    return cfg if isinstance(cfg, dict) else {}


def _udp_dest_ipv4s(dev: dict) -> list[str]:
    """IPv4 UDP destination(s) from discover ``active_config`` (flat and nested)."""
    cfg = _active_config(dev)
    raw: list[str] = []
    for key in ("udp_dest", "udp_dest_zm"):
        v = cfg.get(key)
        if isinstance(v, str) and v.strip():
            raw.append(v.strip())
    for section in ("lidar", "imu", "zm"):
        sec = cfg.get(section)
        if not isinstance(sec, dict):
            continue
        udp = sec.get("udp")
        if isinstance(udp, dict):
            a = udp.get("addr")
            if isinstance(a, str) and a.strip():
                raw.append(a.strip())
    out: list[str] = []
    seen: set[str] = set()
    for item in raw:
        addr = item.partition("/")[0].strip()
        if not addr or addr in seen:
            continue
        try:
            o = ipaddress.ip_address(addr)
        except ValueError:
            continue
        if o.version != 4:
            continue
        seen.add(addr)
        out.append(str(o))
    return out


def _udp_dest_matches_host(dev: dict, host_ips: frozenset[str]) -> bool:
    if not host_ips:
        return False
    return any(d in host_ips for d in _udp_dest_ipv4s(dev))


@functools.lru_cache(maxsize=1)
def _host_ipv4_addresses() -> frozenset[str]:
    """Non-loopback IPv4 addresses on this host (for matching ``udp_dest``)."""
    cands: list[str] = []
    ip_bin = shutil.which("ip")
    if ip_bin:
        r = subprocess.run(
            [ip_bin, "-4", "-o", "addr", "show"],
            capture_output=True,
            text=True,
            check=False,
        )
        if r.returncode == 0:
            for line in r.stdout.splitlines():
                parts = line.split()
                try:
                    i = parts.index("inet")
                    token = parts[i + 1]
                except (ValueError, IndexError):
                    continue
                _append_ipv4(cands, token)
    if not cands:
        hi = shutil.which("hostname")
        if hi:
            r = subprocess.run([hi, "-I"], capture_output=True, text=True, check=False)
            if r.returncode == 0:
                for token in r.stdout.split():
                    _append_ipv4(cands, token)
    if not cands:
        try:
            for info in socket.getaddrinfo(socket.gethostname(), None, socket.AF_INET):
                _append_ipv4(cands, info[4][0])
        except OSError:
            pass
    if not cands:
        try:
            with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as s:
                s.connect(("203.0.113.1", 9))
                _append_ipv4(cands, s.getsockname()[0])
        except OSError:
            pass
    return frozenset(
        ip
        for ip in cands
        if not ipaddress.ip_address(ip).is_loopback
    )


def _prefer_subset(pool: list, pred) -> list:
    """Return *pool* entries matching *pred*, or *pool* unchanged if none match."""
    matched = [d for d in pool if pred(d)]
    return matched if matched else pool


def _auto_pick_candidates(devices: list) -> list:
    """Prefer udp_dest on this host, then RUNNING; else keep prior pool."""
    pool = [d for d in devices if isinstance(d, dict)]
    if not pool:
        return list(devices)
    pool = _prefer_subset(pool, lambda d: _udp_dest_matches_host(d, _host_ipv4_addresses()))
    pool = _prefer_subset(pool, _nested_status_running)
    return pool


def auto_pick(devices: list) -> dict:
    """Host udp_dest match, then RUNNING, then highest parsed fw semver; ties keep order."""
    pool = _auto_pick_candidates(devices)
    if not pool:
        return devices[0]
    best = pool[0]
    best_t = _fw_tuple(best)
    for d in pool[1:]:
        t = _fw_tuple(d)
        if _fw_is_newer(best_t, t):
            best = d
            best_t = t
    return best


def auto_pick_label(dev: dict) -> str:
    si = _sensor_info(dev)
    sn = si.get("prod_sn") or "?"
    hn = (dev.get("hostname") or "").rstrip(".") or connection_target(dev) or "?"
    return f"{hn} (SN {sn})"


def _input_line_timeout(timeout: float) -> str | None:
    """Return one line from stdin, None on timeout or EOF on empty read."""
    ready, _, _ = select.select([sys.stdin], [], [], timeout)
    if not ready:
        return None
    line = sys.stdin.readline()
    if not line:
        return None
    return line.rstrip("\n")


def pick_host(devices: list) -> str:
    auto_dev = auto_pick(devices)
    auto_label = auto_pick_label(auto_dev)
    n = len(devices)
    _tag_line("info", "[*]", "Discovered sensors (firmware image / build):")
    for idx, dev in enumerate(devices, start=1):
        line = format_line(dev)
        if _color_stderr():
            print(
                f"  {_C['muted']}{idx}){_C['reset']} {_C['hl']}{line}{_C['reset']}",
                file=sys.stderr,
                flush=True,
            )
        else:
            print(f"  {idx}) {line}", file=sys.stderr, flush=True)

    if not sys.stdin.isatty():
        _tag_line(
            "info",
            "[*]",
            f"Stdin is not a TTY; using auto-pick ({auto_label}).",
        )
        return connection_target(auto_dev)

    while True:
        _prompt_pick(n, auto_label)
        try:
            choice = _input_line_timeout(AUTO_PICK_S)
        except KeyboardInterrupt:
            print(file=sys.stderr, flush=True)
            _tag_line("warn", "[!]", "Interrupted — scanning again…")
            raise RestartDiscovery from None
        if choice is None:
            print(file=sys.stderr, flush=True)
            _tag_line(
                "info",
                "[*]",
                f"Timeout — auto-selected {auto_label} ({connection_target(auto_dev)}).",
            )
            return connection_target(auto_dev)
        print(file=sys.stderr, flush=True)
        choice = (choice or "").strip()
        if not choice:
            _tag_line(
                "info",
                "[*]",
                f"Empty input — using auto-pick ({auto_label}).",
            )
            return connection_target(auto_dev)
        if not choice.isdigit():
            _tag_line("warn", "[!]", "Invalid choice; try again.")
            continue
        i = int(choice)
        if not 1 <= i <= n:
            _tag_line("warn", "[!]", "Invalid choice; try again.")
            continue
        host = connection_target(devices[i - 1])
        if not host:
            _tag_line("warn", "[!]", "Selected entry has no address; try again.")
            continue
        return host


def main() -> None:
    exe = shutil.which("ouster-cli") or "ouster-cli"
    while True:
        try:
            devices = discover_devices(exe)
            if devices is None:
                _tag_line("bad", "[-]", "Could not parse discover output; retry in 5s...")
                time.sleep(5)
                continue
            if len(devices) < 1:
                _tag_line("bad", "[-]", "No sensors discovered; retry in 5s...")
                time.sleep(5)
                continue
            host = pick_host(devices)
            print(host)
            return
        except RestartDiscovery:
            continue


if __name__ == "__main__":
    main()
