// rMath — native macOS app: a Cocoa window with an embedded WKWebView that
// hosts the bundled notebook, plus the Python server + Rust kernel it drives.
//
//   rMath                open the window (starts the server if needed)
//   rMath --no-window    start the server, wait until healthy, exit
//   rMath --stop         stop a running server
//   rMath --port=9000    preferred port
//
// The server is a child process; quitting the app stops it (and its kernel).

import Cocoa
import WebKit

// ---------------------------------------------------------------- helpers

func resourcesDir() -> URL {
    // .../rMath.app/Contents/MacOS/rMath -> .../Contents/Resources
    let exe = URL(fileURLWithPath: CommandLine.arguments[0]).resolvingSymlinksInPath()
    return exe.deletingLastPathComponent().deletingLastPathComponent()
        .appendingPathComponent("Resources")
}

func appSupportDir() -> URL {
    let bases = FileManager.default.urls(for: .applicationSupportDirectory,
                                         in: .userDomainMask)
    for base in bases {
        let dir = base.appendingPathComponent("rMath")
        if (try? FileManager.default.createDirectory(at: dir,
                                                     withIntermediateDirectories: true)) != nil,
           FileManager.default.isWritableFile(atPath: dir.path) {
            return dir
        }
    }
    // Fall back to a temp dir (e.g. when launched inside a sandbox).
    let fallback = URL(fileURLWithPath: NSTemporaryDirectory())
        .appendingPathComponent("rMath-\(getuid())")
    try? FileManager.default.createDirectory(at: fallback, withIntermediateDirectories: true)
    return fallback
}

func logURL() -> URL {
    let libs = FileManager.default.urls(for: .libraryDirectory, in: .userDomainMask)
    for lib in libs {
        let dir = lib.appendingPathComponent("Logs")
        if (try? FileManager.default.createDirectory(at: dir,
                                                     withIntermediateDirectories: true)) != nil,
           FileManager.default.isWritableFile(atPath: dir.path) {
            return dir.appendingPathComponent("rMath.log")
        }
    }
    return URL(fileURLWithPath: NSTemporaryDirectory()).appendingPathComponent("rMath.log")
}

func pidFileURL() -> URL {
    return appSupportDir().appendingPathComponent("server.pid")
}

func readPidFile() -> pid_t? {
    guard let text = try? String(contentsOf: pidFileURL(), encoding: .utf8) else { return nil }
    return pid_t(text.trimmingCharacters(in: .whitespacesAndNewlines))
}

func writePidFile(_ pid: pid_t) {
    try? String(pid).write(to: pidFileURL(), atomically: true, encoding: .utf8)
}

func removePidFile() {
    try? FileManager.default.removeItem(at: pidFileURL())
}

func killProcess(_ pid: pid_t, signal: Int32) {
    if pid > 0 { kill(pid, signal) }
}

func pythonCandidates() -> [String] {
    var list: [String] = []
    if let p = ProcessInfo.processInfo.environment["RMATH_PYTHON"], !p.isEmpty {
        list.append(p)
    }
    let home = NSHomeDirectory()
    list += [
        "\(home)/miniconda3/bin/python3",
        "\(home)/anaconda3/bin/python3",
        "/opt/homebrew/bin/python3",
        "/usr/local/bin/python3",
        "/usr/bin/python3",
    ]
    return list
}

func usablePython(_ path: String) -> Bool {
    guard FileManager.default.isExecutableFile(atPath: path) else { return false }
    let p = Process()
    p.executableURL = URL(fileURLWithPath: path)
    p.arguments = ["-c", "import http.server, json, subprocess, threading"]
    p.standardOutput = FileHandle.nullDevice
    p.standardError = FileHandle.nullDevice
    do {
        try p.run()
        p.waitUntilExit()
        return p.terminationStatus == 0
    } catch {
        return false
    }
}

func healthOK(_ port: Int) -> Bool {
    guard let url = URL(string: "http://127.0.0.1:\(port)/api/health") else { return false }
    var req = URLRequest(url: url)
    req.timeoutInterval = 1.0
    let sem = DispatchSemaphore(value: 0)
    var ok = false
    URLSession.shared.dataTask(with: req) { data, _, _ in
        ok = (data?.isEmpty == false)
        sem.signal()
    }.resume()
    _ = sem.wait(timeout: .now() + 1.5)
    return ok
}

// ---------------------------------------------------------------- screenshots

func shotRequestURL() -> URL {
    URL(fileURLWithPath: NSTemporaryDirectory()).appendingPathComponent("rMath-shot.request")
}

func shotDoneURL() -> URL {
    URL(fileURLWithPath: NSTemporaryDirectory()).appendingPathComponent("rMath-shot.done")
}

func timestamp() -> String {
    let f = DateFormatter()
    f.dateFormat = "yyyyMMdd-HHmmss"
    return f.string(from: Date())
}

// ---------------------------------------------------------------- app

final class AppDelegate: NSObject, NSApplicationDelegate, NSWindowDelegate,
                        WKNavigationDelegate {
    var window: NSWindow!
    var webView: WKWebView!
    var server: Process?
    var ownsServer = false
    var port = 8787
    var pollTimer: Timer?
    var pollStart = Date()
    var serverLog: FileHandle?
    var shotTimer: Timer?

    // ---- lifecycle

    func applicationDidFinishLaunching(_ note: Notification) {
        NSApp.setActivationPolicy(.regular)
        buildMenu()

        let args = CommandLine.arguments
        if args.contains("--stop") {
            // Stop whatever instance is running: pidfile first, then any
            // leftover notebook/kernel processes.
            if let pid = readPidFile(), pid > 0 {
                killProcess(pid, signal: SIGTERM)
                usleep(500_000)
                killProcess(pid, signal: SIGKILL)
            }
            removePidFile()
            for pattern in ["rmathpy.notebook", "rmath --server"] {
                let p = Process()
                p.executableURL = URL(fileURLWithPath: "/usr/bin/pkill")
                p.arguments = ["-f", pattern]
                try? p.run()
                p.waitUntilExit()
            }
            print("rMath stopped")
            exit(0)
        }
        for a in args where a.hasPrefix("--port=") {
            if let p = Int(a.dropFirst("--port=".count)) { port = p }
        }

        // --screenshot <path>: ask an already-running instance to snapshot
        // itself, then exit. The running app polls the request file and
        // answers through the done file.
        if let idx = args.firstIndex(of: "--screenshot") {
            let dest = idx + 1 < args.count ? args[idx + 1] : ""
            var js = ""
            var delay = 2.5
            if let j = args.firstIndex(of: "--js"), j + 1 < args.count { js = args[j + 1] }
            if let d = args.firstIndex(of: "--delay"), d + 1 < args.count {
                delay = Double(args[d + 1]) ?? 2.5
            }
            exit(requestScreenshot(dest: dest, js: js, delay: delay) ? 0 : 1)
        }

        let headless = args.contains("--no-window")
        if !headless { buildWindow() }
        if !headless {
            startShotWatcher()
            if let p = ProcessInfo.processInfo.environment["RMATH_SHOT_ON_LAUNCH"], !p.isEmpty {
                // handy for scripted verification: shoot once the page settles
                DispatchQueue.main.asyncAfter(deadline: .now() + 8) { self.saveScreenshot(to: p) }
            }
        }

        startServer { [weak self] ok in
            guard let self = self else { return }
            if headless {
                print(ok ? "rMath server healthy on port \(self.port)"
                         : "rMath server FAILED to start (see \(logURL().path))")
                self.stopServer()
                exit(ok ? 0 : 1)
            }
            if ok {
                let url = URL(string: "http://127.0.0.1:\(self.port)")!
                self.webView.load(URLRequest(url: url))
                self.window.title = "rMath — \(url.absoluteString)"
            } else {
                self.showError()
            }
            NSApp.activate(ignoringOtherApps: true)
        }
    }

    func applicationWillTerminate(_ note: Notification) {
        stopServer()
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        return true
    }

    func windowWillClose(_ note: Notification) {
        stopServer()
        NSApp.terminate(nil)
    }

    // ---- window & web view

    func buildWindow() {
        let frame = NSRect(x: 0, y: 0, width: 1120, height: 820)
        window = NSWindow(contentRect: frame,
                          styleMask: [.titled, .closable, .miniaturizable, .resizable],
                          backing: .buffered, defer: false)
        window.title = "rMath"
        window.center()
        window.delegate = self
        window.minSize = NSSize(width: 520, height: 400)

        let config = WKWebViewConfiguration()
        config.websiteDataStore = .nonPersistent()
        webView = WKWebView(frame: frame, configuration: config)
        webView.navigationDelegate = self
        webView.autoresizingMask = [.width, .height]
        window.contentView = webView
        window.makeKeyAndOrderFront(nil)

        webView.loadHTMLString(Self.splashHTML, baseURL: nil)
    }

    static let splashHTML = """
    <!DOCTYPE html><html><head><meta charset="utf-8"><title>rMath</title>
    <style>
      html,body{height:100%;margin:0}
      body{display:flex;align-items:center;justify-content:center;
           font-family:system-ui,-apple-system,sans-serif;background:#fafafa;color:#2c3e50}
      .box{text-align:center}
      .dot{width:12px;height:12px;border-radius:50%;background:#2a58b2;display:inline-block;
           animation:p 1s ease-in-out infinite;margin-right:10px}
      @keyframes p{0%,100%{opacity:.25}50%{opacity:1}}
      h1{font-size:17px;font-weight:600;margin:0 0 6px}
      p{font-size:12px;color:#7a8698;margin:4px 0 0}
    </style></head><body><div class="box">
      <h1><span class="dot"></span>Starting rMath…</h1>
      <p>launching the kernel and notebook server</p>
    </div></body></html>
    """

    func showError() {
        let log = logURL().path
        let html = """
        <!DOCTYPE html><html><head><meta charset="utf-8"><title>rMath</title>
        <style>
          body{font-family:system-ui,-apple-system,sans-serif;background:#fafafa;color:#333;
               padding:40px;line-height:1.5}
          h1{font-size:18px;color:#b43c3c}
          code{background:#eee;padding:2px 6px;border-radius:3px;font-size:12px}
        </style></head><body>
        <h1>The rMath server failed to start</h1>
        <p>Check the log for details:</p>
        <p><code>\(log)</code></p>
        <p>Common causes: no usable <code>python3</code> (set <code>RMATH_PYTHON</code>),
        or the port is in use (try <code>--port=9000</code>).</p>
        </body></html>
        """
        webView.loadHTMLString(html, baseURL: nil)
    }

    // ---- menus

    func buildMenu() {
        let main = NSMenu()

        let appItem = NSMenuItem()
        main.addItem(appItem)
        let appMenu = NSMenu()
        appMenu.addItem(withTitle: "About rMath",
                        action: #selector(NSApplication.orderFrontStandardAboutPanel(_:)),
                        keyEquivalent: "")
        appMenu.addItem(.separator())
        appMenu.addItem(withTitle: "Hide rMath",
                        action: #selector(NSApplication.hide(_:)), keyEquivalent: "h")
        appMenu.addItem(.separator())
        appMenu.addItem(withTitle: "Quit rMath",
                        action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        appItem.submenu = appMenu

        let fileItem = NSMenuItem()
        main.addItem(fileItem)
        let file = NSMenu(title: "File")
        file.addItem(withTitle: "Save Screenshot",
                     action: #selector(shootToDesktop), keyEquivalent: "s")
        fileItem.submenu = file

        let editItem = NSMenuItem()
        main.addItem(editItem)
        let edit = NSMenu(title: "Edit")
        edit.addItem(withTitle: "Undo", action: Selector(("undo:")), keyEquivalent: "z")
        edit.addItem(withTitle: "Redo", action: Selector(("redo:")), keyEquivalent: "Z")
        edit.addItem(.separator())
        edit.addItem(withTitle: "Cut", action: #selector(NSText.cut(_:)), keyEquivalent: "x")
        edit.addItem(withTitle: "Copy", action: #selector(NSText.copy(_:)), keyEquivalent: "c")
        edit.addItem(withTitle: "Paste", action: #selector(NSText.paste(_:)), keyEquivalent: "v")
        edit.addItem(withTitle: "Select All",
                     action: #selector(NSText.selectAll(_:)), keyEquivalent: "a")
        editItem.submenu = edit

        let viewItem = NSMenuItem()
        main.addItem(viewItem)
        let view = NSMenu(title: "View")
        view.addItem(withTitle: "Reload", action: #selector(reloadPage), keyEquivalent: "r")
        view.addItem(withTitle: "Actual Size", action: #selector(zoomReset), keyEquivalent: "0")
        view.addItem(withTitle: "Zoom In", action: #selector(zoomIn), keyEquivalent: "+")
        view.addItem(withTitle: "Zoom Out", action: #selector(zoomOut), keyEquivalent: "-")
        viewItem.submenu = view

        let winItem = NSMenuItem()
        main.addItem(winItem)
        let win = NSMenu(title: "Window")
        win.addItem(withTitle: "Minimize", action: #selector(NSWindow.performMiniaturize(_:)),
                    keyEquivalent: "m")
        win.addItem(withTitle: "Zoom", action: #selector(NSWindow.performZoom(_:)),
                    keyEquivalent: "")
        winItem.submenu = win
        NSApp.windowsMenu = win

        NSApp.mainMenu = main
    }

    @objc func reloadPage() { webView.reload() }
    @objc func zoomReset() { setZoom(1.0) }
    @objc func zoomIn() { setZoom(min(3.0, webView.pageZoom + 0.1)) }
    @objc func zoomOut() { setZoom(max(0.4, webView.pageZoom - 0.1)) }

    func setZoom(_ z: Double) {
        webView.pageZoom = z
    }

    // ---- screenshots

    func startShotWatcher() {
        try? FileManager.default.removeItem(at: shotRequestURL())
        try? FileManager.default.removeItem(at: shotDoneURL())
        shotTimer = Timer.scheduledTimer(withTimeInterval: 0.4, repeats: true) { [weak self] _ in
            self?.pollShotRequest()
        }
    }

    /// Request format (a text file the running app polls):
    ///   line 1: destination path ("" = default Desktop path)
    ///   line 2: optional JavaScript to run first (e.g. evaluate an expression)
    ///   line 3: optional delay in seconds before the snapshot (default 2.5)
    func pollShotRequest() {
        let req = shotRequestURL()
        guard let raw = try? String(contentsOf: req, encoding: .utf8) else { return }
        try? FileManager.default.removeItem(at: req)
        var lines = raw.split(separator: "\n", omittingEmptySubsequences: false).map(String.init)
        let dest = lines.isEmpty ? "" : lines.removeFirst().trimmingCharacters(in: .whitespaces)
        let js = lines.isEmpty ? "" : lines.removeFirst().trimmingCharacters(in: .whitespacesAndNewlines)
        let delay = lines.isEmpty
            ? 2.5
            : (Double(lines.removeFirst().trimmingCharacters(in: .whitespaces)) ?? 2.5)
        let target = dest.isEmpty ? defaultShotPath() : dest
        guard !js.isEmpty, let web = webView else {
            saveScreenshot(to: target)
            return
        }
        web.evaluateJavaScript(js) { _, err in
            if let err = err { print("js error: \(err.localizedDescription)") }
            DispatchQueue.main.asyncAfter(deadline: .now() + delay) {
                self.saveScreenshot(to: target)
            }
        }
    }

    func defaultShotPath() -> String {
        let name = "rMath-\(timestamp()).png"
        if let desktop = FileManager.default.urls(for: .desktopDirectory,
                                                  in: .userDomainMask).first {
            return desktop.appendingPathComponent(name).path
        }
        return URL(fileURLWithPath: NSTemporaryDirectory()).appendingPathComponent(name).path
    }

    /// Snapshot the web view. `.pdf` destinations produce a full page PDF
    /// instead of a viewport PNG. Needs no screen-recording permission: the
    /// app renders its own view.
    func saveScreenshot(to path: String) {
        // Create the destination directory if needed.
        let dir = URL(fileURLWithPath: path).deletingLastPathComponent()
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        guard let web = webView else {
            finishShot("err no window")
            return
        }
        if path.lowercased().hasSuffix(".pdf") {
            let cfg = WKPDFConfiguration()
            cfg.rect = web.bounds
            web.createPDF(configuration: cfg) { result in
                switch result {
                case .success(let data):
                    do {
                        try data.write(to: URL(fileURLWithPath: path))
                        self.finishShot("ok \(path)")
                    } catch { self.finishShot("err \(error.localizedDescription)") }
                case .failure(let err):
                    self.finishShot("err \(err.localizedDescription)")
                }
            }
            return
        }
        let cfg = WKSnapshotConfiguration()
        cfg.rect = web.bounds
        web.takeSnapshot(with: cfg) { image, err in
            guard let image = image,
                  let tiff = image.tiffRepresentation,
                  let rep = NSBitmapImageRep(data: tiff),
                  let png = rep.representation(using: .png, properties: [:]) else {
                self.finishShot("err \(err?.localizedDescription ?? "snapshot failed")")
                return
            }
            do {
                try png.write(to: URL(fileURLWithPath: path))
                self.finishShot("ok \(path)")
            } catch {
                self.finishShot("err \(error.localizedDescription)")
            }
        }
    }

    func finishShot(_ message: String) {
        try? message.write(to: shotDoneURL(), atomically: true, encoding: .utf8)
        print("screenshot: \(message)")
        if message.hasPrefix("ok ") {
            let path = String(message.dropFirst(3))
            NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: path)])
        }
    }

    @objc func shootToDesktop() {
        saveScreenshot(to: defaultShotPath())
    }

    /// Ask the running instance for a screenshot; returns true on success.
    func requestScreenshot(dest: String, js: String = "", delay: Double = 2.5) -> Bool {
        if !healthOK(port) {
            FileHandle.standardError.write(Data(
                "rMath: no running instance on port \(port) to screenshot\n".utf8))
            return false
        }
        try? FileManager.default.removeItem(at: shotDoneURL())
        let target = dest.isEmpty ? URL(fileURLWithPath: NSTemporaryDirectory())
            .appendingPathComponent("rMath-\(timestamp()).png").path : dest
        let request = "\(target)\n\(js.replacingOccurrences(of: "\n", with: " "))\n\(delay)\n"
        do {
            try request.write(to: shotRequestURL(), atomically: true, encoding: .utf8)
        } catch {
            FileHandle.standardError.write(Data("rMath: cannot write request: \(error)\n".utf8))
            return false
        }
        let deadline = Date().addingTimeInterval(20)
        while Date() < deadline {
            if let done = try? String(contentsOf: shotDoneURL(), encoding: .utf8) {
                try? FileManager.default.removeItem(at: shotDoneURL())
                print(done)
                return done.hasPrefix("ok ")
            }
            usleep(150_000)
        }
        FileHandle.standardError.write(Data("rMath: screenshot timed out\n".utf8))
        return false
    }

    // ---- server management

    func startServer(done: @escaping (Bool) -> Void) {
        // Already running (e.g. started from the CLI)? Attach to it.
        if healthOK(port) {
            ownsServer = false
            done(true)
            return
        }

        guard let python = pythonCandidates().first(where: usablePython) else {
            FileHandle.standardError.write(Data("rMath: no usable python3 found\n".utf8))
            done(false)
            return
        }

        let res = resourcesDir()
        let work = appSupportDir().appendingPathComponent("work")
        try? FileManager.default.createDirectory(at: work, withIntermediateDirectories: true)
        // Seed the writable work dir with the bundled demo files.
        let demoIn = res.appendingPathComponent("demo")
        let demoOut = work.appendingPathComponent("demo")
        try? FileManager.default.createDirectory(at: demoOut, withIntermediateDirectories: true)
        // Always refresh the bundled demo sources (demo.mt, data.csv) so app
        // updates take effect; generated outputs are left in place.
        if let sources = try? FileManager.default.contentsOfDirectory(
            at: demoIn, includingPropertiesForKeys: nil) {
            for src in sources {
                let dst = demoOut.appendingPathComponent(src.lastPathComponent)
                try? FileManager.default.removeItem(at: dst)
                try? FileManager.default.copyItem(at: src, to: dst)
            }
        }

        // Find a free port: try the preferred one, then a few neighbours.
        var chosen = port
        for candidate in stride(from: port, to: port + 12, by: 1) {
            if !healthOK(candidate) && !portInUse(candidate) {
                chosen = candidate
                break
            }
        }
        port = chosen

        let log = logURL()
        if !FileManager.default.fileExists(atPath: log.path) {
            FileManager.default.createFile(atPath: log.path, contents: nil)
        }
        serverLog = try? FileHandle(forWritingTo: log)
        serverLog?.seekToEndOfFile()
        let header = "--- rMath window launch \(Date()) python=\(python) port=\(port) workdir=\(work.path)\n"
        serverLog?.write(Data(header.utf8))

        let task = Process()
        task.executableURL = URL(fileURLWithPath: python)
        task.arguments = ["-u", "-m", "rmathpy.notebook", "--port", String(port)]
        task.currentDirectoryURL = work
        var env = ProcessInfo.processInfo.environment
        env["PYTHONPATH"] = res.path
        env["RMATH_BIN"] = res.appendingPathComponent("rmath").path
        env["RMATH_WORKDIR"] = work.path
        env["PYTHONUNBUFFERED"] = "1"
        task.environment = env
        if let h = serverLog {
            task.standardOutput = h
            task.standardError = h
        }
        do {
            try task.run()
        } catch {
            FileHandle.standardError.write(Data("rMath: could not start server: \(error)\n".utf8))
            done(false)
            return
        }
        server = task
        ownsServer = true
        writePidFile(task.processIdentifier)

        // Poll for readiness on the main run loop (localhost checks are cheap).
        pollStart = Date()
        pollTimer?.invalidate()
        pollTimer = Timer.scheduledTimer(withTimeInterval: 0.2, repeats: true) { [weak self] t in
            guard let self = self else { return }
            if healthOK(self.port) {
                t.invalidate()
                done(true)
            } else if Date().timeIntervalSince(self.pollStart) > 25 {
                t.invalidate()
                done(false)
            }
        }
    }

    func portInUse(_ p: Int) -> Bool {
        // A quick TCP connect test (no health endpoint required).
        var hints = addrinfo()
        hints.ai_family = AF_INET
        hints.ai_socktype = SOCK_STREAM
        hints.ai_protocol = IPPROTO_TCP
        var info: UnsafeMutablePointer<addrinfo>?
        guard getaddrinfo("127.0.0.1", String(p), &hints, &info) == 0, let res = info else {
            return false
        }
        defer { freeaddrinfo(info) }
        let fd = socket(res.pointee.ai_family, res.pointee.ai_socktype, res.pointee.ai_protocol)
        if fd < 0 { return false }
        defer { close(fd) }
        return connect(fd, res.pointee.ai_addr, res.pointee.ai_addrlen) == 0
    }

    func stopServer() {
        pollTimer?.invalidate()
        pollTimer = nil
        shotTimer?.invalidate()
        shotTimer = nil
        guard ownsServer, let task = server else { return }
        if task.isRunning {
            task.terminate()                     // SIGTERM -> server closes kernel
            let deadline = Date().addingTimeInterval(4)
            while task.isRunning && Date() < deadline {
                usleep(100_000)
            }
            if task.isRunning { kill(task.processIdentifier, SIGKILL) }
        }
        server = nil
        ownsServer = false
        removePidFile()
        // Belt and braces: no orphaned kernel.
        let pkill = Process()
        pkill.executableURL = URL(fileURLWithPath: "/usr/bin/pkill")
        pkill.arguments = ["-f", "rmath --server"]
        try? pkill.run()
        pkill.waitUntilExit()
        serverLog?.closeFile()
        serverLog = nil
    }
}

// ---------------------------------------------------------------- main

let app = NSApplication.shared
let delegate = AppDelegate()
app.delegate = delegate
app.run()
