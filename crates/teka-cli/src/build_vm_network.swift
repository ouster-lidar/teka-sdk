import Foundation
import Virtualization

// Retain the socket across on-demand boots: gvproxy remembers its peer's address.
final class HostNetwork {
  static let shared = HostNetwork()
  private let proxy = Process()
  private var directory = ""
  private var socket: FileHandle?

  func configure(_ network: VZVirtioNetworkDeviceConfiguration) throws {
    if socket == nil { try start() }
    network.attachment = VZFileHandleNetworkDeviceAttachment(fileHandle: socket!)
    // Match gvproxy's static DHCP lease across guest boots.
    network.macAddress = VZMACAddress(string: "5a:94:ef:e4:0c:ee")!
  }

  private func start() throws {
    // Darwin's Unix socket paths are limited to 104 bytes; TMPDIR can be longer.
    var template = Array("/tmp/teka-vm-net.XXXXXX".utf8CString)
    guard mkdtemp(&template) != nil else {
      throw VZVMError.runtime("cannot create network socket directory")
    }
    directory = String(cString: template)
    atexit { HostNetwork.shared.stop() }

    proxy.executableURL = URL(fileURLWithPath: "@gvproxy@")
    proxy.arguments = ["-listen-vfkit", "unixgram://\(directory)/proxy.sock", "-ssh-port", "-1"]
    try proxy.run()
    proxy.terminationHandler = { process in
      Log.error("network proxy exited (\(process.terminationStatus)); stopping VM runner")
      kill(getpid(), SIGTERM)
    }

    let deadline = Date().addingTimeInterval(5)
    while !FileManager.default.fileExists(atPath: "\(directory)/proxy.sock") {
      guard proxy.isRunning, Date() < deadline else {
        throw VZVMError.runtime("network proxy did not create its socket within 5s")
      }
      Thread.sleep(forTimeInterval: 0.005)
    }

    let fd = Darwin.socket(AF_UNIX, SOCK_DGRAM, 0)
    guard fd >= 0 else { throw VZVMError.runtime("cannot create network socket") }
    let handle = FileHandle(fileDescriptor: fd, closeOnDealloc: true)
    try address("\(directory)/guest.sock") { Darwin.bind(fd, $0, $1) }
    try address("\(directory)/proxy.sock") { Darwin.connect(fd, $0, $1) }
    for (option, capacity) in [(SO_SNDBUF, 1024 * 1024), (SO_RCVBUF, 4 * 1024 * 1024)] {
      var size = Int32(capacity)
      guard setsockopt(fd, SOL_SOCKET, option, &size, socklen_t(MemoryLayout<Int32>.size)) == 0 else {
        throw VZVMError.runtime("cannot size network socket buffers")
      }
    }
    socket = handle
  }

  private func address(_ path: String, operation: (UnsafePointer<sockaddr>, socklen_t) -> Int32) throws {
    var address = sockaddr_un()
    address.sun_family = sa_family_t(AF_UNIX)
    address.sun_len = UInt8(MemoryLayout<sockaddr_un>.size)
    let bytes = Array(path.utf8CString)
    withUnsafeMutableBytes(of: &address.sun_path) { destination in
      bytes.withUnsafeBytes { destination.copyMemory(from: $0) }
    }
    let result = withUnsafePointer(to: &address) {
      $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
        operation($0, socklen_t(MemoryLayout<sockaddr_un>.size))
      }
    }
    guard result == 0 else {
      throw VZVMError.runtime("network socket \(path): \(String(cString: strerror(errno)))")
    }
  }

  private func stop() {
    proxy.terminationHandler = nil
    if proxy.isRunning {
      proxy.terminate()
      proxy.waitUntilExit()
    }
    try? socket?.close()
    try? FileManager.default.removeItem(atPath: directory)
  }
}
