# TekaOS 0.1

TekaOS 0.1 is our first release. It runs on the ZED Box Mini and the ZED Box 2.

## 0.1.0

### What's new

- A read-only system with safe updates that roll back on their own.
- Install from a USB drive.
- NVIDIA JetPack 7.2 and CUDA 13.2 on Jetson devices.
- A web interface with a password.
- Install and manage apps, with GPU and screen access.
- The Teka CLI: build apps, deploy them to many boxes at once, and update TekaOS.
- Build apps on a Mac with the Teka Linux build VM.
- Boxes name themselves after their serial number, and you can rename them.
- System utilities: General, Sensors, Network, Time Sync, Network Diagnostics, UDP Relay, Packet Monitor and Simulator, and System Update. DHCP for sensors is on Network.

### Known issues

- Update files aren't signed yet.
- A factory reset removes every app, including the system apps.
- The Sensors point cloud view isn't ready yet.
