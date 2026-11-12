import os
import socket
import sys
import time


directory, *command = sys.argv[1:]
deadline = time.monotonic() + 5
while not os.path.exists(f"{directory}/proxy.sock"):
    if time.monotonic() >= deadline:
        sys.exit("gvproxy did not create its socket within 5s")
    time.sleep(0.01)

network = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
network.bind(f"{directory}/guest.sock")
network.connect(f"{directory}/proxy.sock")
network.setsockopt(socket.SOL_SOCKET, socket.SO_SNDBUF, 1024 * 1024)
network.setsockopt(socket.SOL_SOCKET, socket.SO_RCVBUF, 4 * 1024 * 1024)
network.set_inheritable(True)
os.execvp(command[0], [*command, f"--net-fd={network.fileno()}"])
