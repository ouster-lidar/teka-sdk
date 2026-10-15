"""OSF frame stream over a Unix-domain socket (peer FrameSet IPC).

Wire format per message:
  ``u32`` little-endian payload length + single-frame OSF file bytes
  (one :class:`~ouster.sdk.core.FrameSet`, including ``objects`` when present).

Producers bind with :class:`OsfSocketPublisher`. Consumers call
:func:`open_source` in this module (wraps the SDK for ``.sock`` / ``osf+unix://``).
"""

from __future__ import annotations

import mmap
import os
import queue
import socket
import struct
import tempfile
import threading
import time
from pathlib import Path
from typing import Callable, Iterator, List, Optional, Tuple, Union

from ouster.sdk._bindings.client import FrameSetSource
from ouster.sdk.core import FrameSet, SensorInfo
from ouster.sdk.osf import OsfFrameSetSource

_HDR = struct.Struct("<I")
_MAX_FRAME_BYTES = 64 * 1024 * 1024
_OSF_UNIX_PREFIX = "osf+unix://"
_STREAM_END = object()


def _osf_temp_dir() -> str:
    shm = "/dev/shm"
    if os.path.isdir(shm):
        return shm
    return tempfile.gettempdir()


class _MemfdFrameBuffer:
    """Reusable memfd (or /dev/shm fallback) backing store for socket OSF blobs."""

    def __init__(self) -> None:
        self._fd: Optional[int] = None
        self._mmap: Optional[mmap.mmap] = None
        self._path: Optional[str] = None
        self._shm_path: Optional[str] = None
        self._size = 0

    @property
    def path(self) -> Optional[str]:
        return self._path

    def ensure(self, size: int) -> mmap.mmap:
        if self._mmap is not None and self._size == size:
            return self._mmap
        self._drop_mmap()
        if self._fd is None:
            try:
                fd = os.memfd_create("osf-socket-frame")
                self._path = f"/proc/self/fd/{fd}"
                self._fd = fd
            except (AttributeError, OSError):
                shm_path = os.path.join(
                    _osf_temp_dir(), f"osf-socket-frame-{os.getpid()}.osf"
                )
                fd = os.open(shm_path, os.O_CREAT | os.O_RDWR, 0o600)
                self._shm_path = shm_path
                self._path = shm_path
                self._fd = fd
        os.ftruncate(self._fd, size)
        self._mmap = mmap.mmap(self._fd, size)
        self._size = size
        return self._mmap

    def flush_for_read(self) -> None:
        if self._mmap is not None:
            self._mmap.flush()
        if self._fd is not None:
            os.lseek(self._fd, 0, os.SEEK_SET)

    def _drop_mmap(self) -> None:
        if self._mmap is not None:
            try:
                self._mmap.close()
            except OSError:
                pass
            self._mmap = None
        self._size = 0

    def close(self) -> None:
        self._drop_mmap()
        if self._fd is not None:
            try:
                os.close(self._fd)
            except OSError:
                pass
            self._fd = None
        if self._shm_path is not None:
            try:
                os.unlink(self._shm_path)
            except OSError:
                pass
            self._shm_path = None
        self._path = None


def _write_blob_to_memfd(blob: bytes) -> Tuple[str, _MemfdFrameBuffer]:
    buf = _MemfdFrameBuffer()
    mm = buf.ensure(len(blob))
    mm[: len(blob)] = blob
    assert buf.path is not None
    return buf.path, buf


def normalize_osf_socket_path(url: Union[str, List[str]]) -> str:
    """Strip ``osf+unix://`` if present; accept a single path only."""
    if isinstance(url, list):
        if len(url) != 1:
            raise ValueError("OSF_SOCKET source accepts exactly one path")
        url = url[0]
    path = os.path.expanduser(str(url))
    if path.startswith(_OSF_UNIX_PREFIX):
        path = path[len(_OSF_UNIX_PREFIX) :]
        # Allow osf+unix:///abs/path → /abs/path
        if path.startswith("//"):
            path = path[1:]
    return path


def encode_frameset_osf(frame_set: FrameSet, sensor_info) -> bytes:
    """Serialize one FrameSet (incl. ``objects``) to a single-frame OSF blob."""
    from ouster.sdk import osf

    infos = list(sensor_info) if not isinstance(sensor_info, list) else sensor_info
    fd, path = tempfile.mkstemp(suffix=".osf")
    os.close(fd)
    try:
        with osf.Writer(path, infos) as writer:
            writer.save(frame_set)
        blob = Path(path).read_bytes()
        if len(blob) < 64:
            raise RuntimeError(f"OSF encode produced tiny blob ({len(blob)} bytes)")
        return blob
    finally:
        try:
            os.unlink(path)
        except OSError:
            pass


def decode_frameset_osf(blob: bytes):
    """Return ``(FrameSet, sensor_info)`` from a single-frame OSF blob."""
    path, owned_buf = _write_blob_to_memfd(blob)
    try:
        src = OsfFrameSetSource(path, index=False)
        try:
            for frame_set in src:
                return frame_set, list(src.sensor_info)
        finally:
            src.close()
        raise RuntimeError("empty OSF FrameSet blob")
    finally:
        owned_buf.close()


class OsfSocketPublisher:
    """Publish FrameSets on an AF_UNIX stream socket (latest-wins, one client)."""

    def __init__(self, path: str) -> None:
        self.path = normalize_osf_socket_path(path)
        self._sock: Optional[socket.socket] = None
        self._client: Optional[socket.socket] = None
        self._lock = threading.Lock()
        self._stop = threading.Event()
        self._thread: Optional[threading.Thread] = None
        self._latest: Optional[bytes] = None

    def start(self) -> None:
        parent = os.path.dirname(self.path)
        if parent:
            os.makedirs(parent, exist_ok=True)
        try:
            if os.path.exists(self.path):
                os.unlink(self.path)
        except OSError:
            pass
        srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        srv.bind(self.path)
        try:
            os.chmod(self.path, 0o666)
        except OSError:
            pass
        srv.listen(1)
        srv.settimeout(0.5)
        self._sock = srv
        self._thread = threading.Thread(
            target=self._accept_loop, name="osf-socket-accept", daemon=True
        )
        self._thread.start()

    def _accept_loop(self) -> None:
        assert self._sock is not None
        while not self._stop.is_set():
            try:
                conn, _ = self._sock.accept()
            except socket.timeout:
                continue
            except OSError:
                break
            conn.settimeout(2.0)
            with self._lock:
                old = self._client
                self._client = conn
            if old is not None:
                try:
                    old.close()
                except OSError:
                    pass
            with self._lock:
                blob = self._latest
                if blob:
                    self._send_raw_locked(conn, blob)

    def _send_raw_locked(self, conn: socket.socket, blob: bytes) -> bool:
        try:
            conn.sendall(_HDR.pack(len(blob)) + blob)
            return True
        except OSError:
            if self._client is conn:
                self._client = None
            try:
                conn.close()
            except OSError:
                pass
            return False

    def publish(self, frame_set: FrameSet, sensor_info) -> None:
        """Encode and send (or stash) one FrameSet for the connected client."""
        try:
            blob = encode_frameset_osf(frame_set, sensor_info)
        except Exception as exc:  # noqa: BLE001
            raise RuntimeError(f"OsfSocketPublisher encode failed: {exc!r}") from exc
        if len(blob) > _MAX_FRAME_BYTES:
            raise RuntimeError(
                f"OsfSocketPublisher: frame too large ({len(blob)} bytes)"
            )
        with self._lock:
            self._latest = blob
            conn = self._client
            if conn is not None:
                self._send_raw_locked(conn, blob)

    def close(self) -> None:
        self._stop.set()
        with self._lock:
            if self._client is not None:
                try:
                    self._client.close()
                except OSError:
                    pass
                self._client = None
        if self._sock is not None:
            try:
                self._sock.close()
            except OSError:
                pass
            self._sock = None
        if self._thread is not None:
            self._thread.join(timeout=2.0)
            self._thread = None
        try:
            if os.path.exists(self.path):
                os.unlink(self.path)
        except OSError:
            pass

    def __enter__(self) -> "OsfSocketPublisher":
        self.start()
        return self

    def __exit__(self, *args) -> None:
        self.close()


class OsfSocketFrameSetSource(FrameSetSource):
    """Live :class:`FrameSetSource` reading an OSF frame stream from a Unix socket."""

    def __init__(
        self,
        path: Union[str, List[str]],
        *,
        connect_timeout_s: Optional[float] = None,
        **kwargs,
    ) -> None:
        """
        Args:
            path: Filesystem path or ``osf+unix://`` URI to the publisher socket.
            connect_timeout_s: Optional bound on waiting for the first FrameSet
                (``None`` waits indefinitely).
        """
        FrameSetSource.__init__(self)
        _ = kwargs  # open_source may forward unused FrameSetSourceOptions
        self.path = normalize_osf_socket_path(path)
        self._connect_timeout_s = connect_timeout_s
        self._sensor_info: List[SensorInfo] = []
        self._sock: Optional[socket.socket] = None
        self._closed = False
        self._q: queue.Queue = queue.Queue(maxsize=1)
        self._last: Optional[FrameSet] = None
        self._had_live_stream = False
        self._stream_ended = False
        self._reader_error: Optional[BaseException] = None
        self._on_disconnect: Optional[Callable[[], None]] = None
        self._bufs = [_MemfdFrameBuffer(), _MemfdFrameBuffer(), _MemfdFrameBuffer()]
        self._blob_lock = threading.Lock()
        self._blob_cv = threading.Condition(self._blob_lock)
        self._pending: Optional[Tuple[int, int]] = None  # (buf_index, nbytes)
        self._decoding_i = -1
        self._recv_thread = threading.Thread(
            target=self._recv_loop, name="osf-socket-recv", daemon=True
        )
        self._thread = threading.Thread(
            target=self._reader_loop, name="osf-socket-decode", daemon=True
        )
        self._recv_thread.start()
        self._thread.start()
        print(
            "osf-socket: memfd decode (recv thread, latest-wins if consumer busy)",
            flush=True,
        )
        self._wait_first(timeout_s=connect_timeout_s)

    def set_on_disconnect(self, cb: Optional[Callable[[], None]]) -> None:
        """Optional callback when a live peer drops (e.g. stop PointViz)."""
        self._on_disconnect = cb

    @property
    def is_live(self) -> bool:
        return True

    @property
    def sensor_info(self) -> List[SensorInfo]:
        return self._sensor_info

    def _wait_first(self, timeout_s: Optional[float] = None) -> FrameSet:
        deadline = (
            None if timeout_s is None else (time.monotonic() + float(timeout_s))
        )
        while not self._closed:
            if self._sensor_info and self._last is not None:
                return self._last
            if self._reader_error is not None and self._last is None:
                raise self._reader_error
            if deadline is not None:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise TimeoutError(
                        f"no OSF socket publisher at {self.path} after {timeout_s:.0f}s"
                    )
                wait = min(0.5, remaining)
            else:
                wait = 0.5
            try:
                frame = self._q.get(timeout=wait)
            except queue.Empty:
                continue
            if frame is _STREAM_END:
                continue
            self._last = frame
            self._had_live_stream = True
            return frame
        raise RuntimeError(f"OsfSocketFrameSetSource closed before first frame ({self.path})")

    def _connect(self) -> socket.socket:
        while not self._closed:
            if not os.path.exists(self.path):
                time.sleep(0.5)
                continue
            sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            try:
                sock.connect(self.path)
                sock.settimeout(2.0)
                return sock
            except OSError:
                try:
                    sock.close()
                except OSError:
                    pass
                time.sleep(0.5)
        raise StopIteration

    def _recv_into(self, sock: socket.socket, dest: memoryview, n: int) -> None:
        filled = 0
        while filled < n:
            try:
                got = sock.recv_into(dest[filled:n])
            except socket.timeout:
                continue
            if got == 0:
                raise ConnectionError("OSF socket peer closed")
            filled += got

    def _recv_blob_into(self, buf: _MemfdFrameBuffer) -> int:
        if self._sock is None:
            self._sock = self._connect()
        assert self._sock is not None
        hdr = bytearray(_HDR.size)
        self._recv_into(self._sock, memoryview(hdr), _HDR.size)
        (n,) = _HDR.unpack(hdr)
        if n <= 0 or n > _MAX_FRAME_BYTES:
            raise ConnectionError(f"bad OSF socket frame length {n}")
        mm = buf.ensure(n)
        self._recv_into(self._sock, memoryview(mm)[:n], n)
        return n

    def _pick_recv_index(self) -> int:
        used = set()
        if self._pending is not None:
            used.add(self._pending[0])
        if self._decoding_i >= 0:
            used.add(self._decoding_i)
        for i in range(len(self._bufs)):
            if i not in used:
                return i
        return 0

    def _recv_loop(self) -> None:
        while not self._closed:
            try:
                with self._blob_lock:
                    idx = self._pick_recv_index()
                n = self._recv_blob_into(self._bufs[idx])
                with self._blob_cv:
                    self._pending = (idx, n)
                    self._blob_cv.notify()
            except StopIteration:
                break
            except socket.timeout:
                continue
            except (OSError, ConnectionError) as exc:
                if self._closed:
                    break
                if getattr(exc, "errno", None) == 9:
                    self._close_sock()
                    time.sleep(0.1)
                    continue
                if self._had_live_stream:
                    self._end_live_stream(exc)
                    break
                self._reader_error = None
                self._close_sock()
                time.sleep(0.5)
            except Exception as exc:  # noqa: BLE001
                if self._closed:
                    break
                if self._had_live_stream:
                    self._end_live_stream(exc)
                    break
                self._reader_error = exc
                self._close_sock()
                time.sleep(0.5)

    def _decode_from_buf(self, buf: _MemfdFrameBuffer) -> FrameSet:
        path = buf.path
        assert path is not None
        buf.flush_for_read()
        src = OsfFrameSetSource(path, index=False)
        try:
            frame_set = None
            infos: List[SensorInfo] = []
            for frame_set in src:
                infos = list(src.sensor_info)
                break
            if frame_set is None:
                raise RuntimeError("empty OSF FrameSet blob")
        finally:
            src.close()
        # Keep the same SensorInfo object identities so SimpleViz (which holds
        # the list from open time) sees live extrinsic / metadata updates from
        # each OSF blob (e.g. Dynamo live-config sensor_to_body changes).
        if self._sensor_info and len(self._sensor_info) == len(infos):
            for dst, src_si in zip(self._sensor_info, infos):
                try:
                    dst.sensor_to_body = src_si.sensor_to_body
                except Exception:
                    pass
        else:
            self._sensor_info = infos
        return frame_set

    def _close_sock(self) -> None:
        if self._sock is None:
            return
        try:
            self._sock.close()
        except OSError:
            pass
        self._sock = None

    def _end_live_stream(self, _reason: BaseException) -> None:
        if self._stream_ended:
            return
        self._stream_ended = True
        self._reader_error = None
        self._close_sock()
        with self._blob_cv:
            self._blob_cv.notify_all()
        try:
            self._q.get_nowait()
        except queue.Empty:
            pass
        try:
            self._q.put_nowait(_STREAM_END)
        except queue.Full:
            pass
        cb = self._on_disconnect
        if cb is not None:
            try:
                cb()
            except Exception:
                pass

    def _reader_loop(self) -> None:
        while not self._closed and not self._stream_ended:
            try:
                with self._blob_cv:
                    while self._pending is None and not self._closed and not self._stream_ended:
                        self._blob_cv.wait(timeout=0.5)
                    if self._pending is None:
                        continue
                    idx, _nbytes = self._pending
                    self._pending = None
                    if not self._q.empty():
                        continue
                    self._decoding_i = idx
                frame = self._decode_from_buf(self._bufs[idx])
                with self._blob_lock:
                    self._decoding_i = -1
                self._reader_error = None
                self._had_live_stream = True
                try:
                    self._q.get_nowait()
                except queue.Empty:
                    pass
                try:
                    self._q.put_nowait(frame)
                except queue.Full:
                    pass
                self._last = frame
            except Exception as exc:  # noqa: BLE001
                with self._blob_lock:
                    self._decoding_i = -1
                if self._closed:
                    break
                if self._had_live_stream:
                    self._end_live_stream(exc)
                    break
                self._reader_error = exc
                time.sleep(0.5)

    def __iter__(self) -> Iterator[FrameSet]:
        if self._last is not None:
            yield self._last
            self._last = None
        while not self._closed and not self._stream_ended:
            try:
                item = self._q.get(timeout=0.5)
            except queue.Empty:
                if self._stream_ended:
                    break
                continue
            if item is _STREAM_END:
                break
            yield item

    def close(self) -> None:
        self._closed = True
        self._close_sock()
        with self._blob_cv:
            self._blob_cv.notify_all()
        for th in (self._recv_thread, self._thread):
            if th is not None and th.is_alive():
                th.join(timeout=2.0)
        for buf in self._bufs:
            buf.close()

    def __enter__(self) -> "OsfSocketFrameSetSource":
        return self

    def __exit__(self, *args) -> None:
        self.close()


def looks_like_osf_socket(url: Union[str, List[str]]) -> bool:
    if isinstance(url, list):
        if not url:
            return False
        url = url[0]
    s = str(url)
    if s.startswith(_OSF_UNIX_PREFIX):
        return True
    try:
        return normalize_osf_socket_path(s).endswith(".sock")
    except Exception:
        return str(s).endswith(".sock")


def open_source(source_url, collate=True, sensor_idx=-1, *args, **kwargs):
    """Like ``ouster.sdk.open_source``, plus peer Unix-socket FrameSet paths."""
    if looks_like_osf_socket(source_url):
        src = OsfSocketFrameSetSource(source_url, *args, **kwargs)
        if sensor_idx >= 0:
            src = src.single(sensor_idx)
        return src
    from ouster.sdk import open_source as sdk_open_source

    return sdk_open_source(
        source_url, collate=collate, sensor_idx=sensor_idx, *args, **kwargs
    )
