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

/// Where bug reports go: the project's .shots/reports when the install recorded
/// a workspace path (so the agent can read them), else ~/Desktop/rMath-reports.
func reportDir() -> URL {
    let cfg = appSupportDir().appendingPathComponent("workspace")
    if let ws = try? String(contentsOf: cfg, encoding: .utf8) {
        let trimmed = ws.trimmingCharacters(in: .whitespacesAndNewlines)
        if !trimmed.isEmpty, FileManager.default.fileExists(atPath: trimmed) {
            let dir = URL(fileURLWithPath: trimmed).appendingPathComponent(".shots/reports")
            if (try? FileManager.default.createDirectory(at: dir,
                                                         withIntermediateDirectories: true)) != nil {
                return dir
            }
        }
    }
    let desktop = FileManager.default.urls(for: .desktopDirectory, in: .userDomainMask).first
        ?? URL(fileURLWithPath: NSTemporaryDirectory())
    let dir = desktop.appendingPathComponent("rMath-reports")
    try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    return dir
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

// ---------------------------------------------------------------- palette button

/// A palette button drawn by hand: NSButton's system appearance does not
/// render into an offscreen bitmap, and palettes are snapshot for
/// verification. Looks like an AppKit push button, behaves like one.
final class PaletteButton: NSView {
    let label: String
    let insertText: String
    weak var target: AnyObject?
    var action: Selector?
    private var highlighted = false

    init(frame: NSRect, label: String, insertText: String) {
        self.label = label
        self.insertText = insertText
        super.init(frame: frame)
        self.toolTip = "Insert " + label
    }

    required init?(coder: NSCoder) { fatalError("not used") }

    override func draw(_ dirty: NSRect) {
        let r = bounds.insetBy(dx: 1, dy: 1)
        let path = NSBezierPath(roundedRect: r, xRadius: 4, yRadius: 4)
        if highlighted {
            NSColor.selectedControlColor.setFill()
        } else {
            NSColor.controlBackgroundColor.setFill()
        }
        path.fill()
        (highlighted ? NSColor.controlAccentColor : NSColor.gridColor).setStroke()
        path.lineWidth = 1
        path.stroke()
        let attrs: [NSAttributedString.Key: Any] = [
            .font: NSFont.monospacedSystemFont(ofSize: 11.5, weight: .regular),
            .foregroundColor: NSColor.labelColor,
        ]
        let text = label as NSString
        let size = text.size(withAttributes: attrs)
        text.draw(at: NSPoint(x: bounds.midX - size.width / 2,
                              y: bounds.midY - size.height / 2),
                  withAttributes: attrs)
    }

    override func mouseDown(with event: NSEvent) {
        highlighted = true
        needsDisplay = true
    }

    override func mouseUp(with event: NSEvent) {
        highlighted = false
        needsDisplay = true
        let p = convert(event.locationInWindow, from: nil)
        if bounds.contains(p), let action = action {
            _ = NSApp.sendAction(action, to: target, from: self)
        }
    }

    override func resetCursorRects() {
        addCursorRect(bounds, cursor: .pointingHand)
    }
}

// ---------------------------------------------------------------- app

final class AppDelegate: NSObject, NSApplicationDelegate, NSWindowDelegate,
                        WKNavigationDelegate, WKUIDelegate, WKScriptMessageHandler {
    var window: NSWindow!
    var webView: WKWebView!
    var server: Process?
    var ownsServer = false
    var restarting = false
    var watchTimer: Timer?
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

        // --dump-menus: print the menu tree (verifiable without UI scripting).
        if args.contains("--dump-menus") {
            func mods(_ item: NSMenuItem) -> String {
                let m = item.keyEquivalentModifierMask
                var out = ""
                if m.contains(.control) { out += "^" }
                if m.contains(.option) { out += "\u{2325}" }
                if m.contains(.shift) { out += "\u{21e7}" }
                if m.contains(.command) { out += "\u{2318}" }
                return out
            }
            func dump(_ menu: NSMenu?, _ depth: Int) {
                guard let menu = menu else { return }
                for item in menu.items {
                    if item.isSeparatorItem {
                        print(String(repeating: "  ", count: depth) + "---")
                        continue
                    }
                    let key = item.keyEquivalent.isEmpty
                        ? "" : "   [\(mods(item))\(item.keyEquivalent)]"
                    print(String(repeating: "  ", count: depth) + item.title + key)
                    if item.submenu != nil { dump(item.submenu, depth + 1) }
                }
            }
            dump(NSApp.mainMenu, 0)
            exit(0)
        }

        if args.contains("--report") {
            exit(requestReport() ? 0 : 1)
        }

        if let idx = args.firstIndex(of: "--screenshot-palette") {
            let name = idx + 1 < args.count ? args[idx + 1] : "Basic Math"
            let out = idx + 2 < args.count ? args[idx + 2] : "/tmp/palette.png"
            guard let defs = AppDelegate.paletteDefs.first(where: { $0.0 == name })?.1 else {
                FileHandle.standardError.write(Data("unknown palette\n".utf8))
                exit(2)
            }
            let view = paletteView(named: name, defs: defs)
            let holder = NSWindow(contentRect: view.frame,
                                  styleMask: [.titled, .closable, .utilityWindow],
                                  backing: .buffered, defer: false)
            holder.contentView = view
            // Order it on screen for a moment: AppKit only draws windows that
            // have actually been displayed, so the snapshot needs a draw pass.
            holder.makeKeyAndOrderFront(nil)
            holder.displayIfNeeded()
            RunLoop.current.run(until: Date().addingTimeInterval(0.5))
            view.layoutSubtreeIfNeeded()
            view.displayIfNeeded()
            view.wantsLayer = true
            guard let layer = view.layer else { exit(1) }
            let w = Int(view.bounds.width), h = Int(view.bounds.height)
            guard let rep = NSBitmapImageRep(
                bitmapDataPlanes: nil, pixelsWide: w, pixelsHigh: h,
                bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0) else {
                exit(1)
            }
            rep.size = NSSize(width: view.bounds.width, height: view.bounds.height)
            NSGraphicsContext.saveGraphicsState()
            if let ctx = NSGraphicsContext(bitmapImageRep: rep) {
                NSGraphicsContext.current = ctx
                layer.render(in: ctx.cgContext)
            }
            NSGraphicsContext.restoreGraphicsState()
            holder.orderOut(nil)
            if let png = rep.representation(using: .png, properties: [:]) {
                try? png.write(to: URL(fileURLWithPath: out))
                print("ok \(out)")
                exit(0)
            }
            exit(1)
        }

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
            var what = "page"
            if let j = args.firstIndex(of: "--js"), j + 1 < args.count { js = args[j + 1] }
            if let d = args.firstIndex(of: "--delay"), d + 1 < args.count {
                delay = Double(args[d + 1]) ?? 2.5
            }
            if let p = args.firstIndex(of: "--palette"), p + 1 < args.count {
                what = "palette:" + args[p + 1]
            }
            exit(requestScreenshot(dest: dest, js: js, delay: delay, what: what) ? 0 : 1)
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
                self.startServerWatchdog()
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
        window.title = "Untitled-1 — rMath"
        window.center()
        window.delegate = self
        window.minSize = NSSize(width: 520, height: 400)

        let config = WKWebViewConfiguration()
        config.websiteDataStore = .nonPersistent()
        // The page talks to the app through this handler (no custom URL scheme,
        // which the system would otherwise try to open with LaunchServices).
        config.userContentController.add(self, name: "rmath")
        webView = WKWebView(frame: frame, configuration: config)
        webView.navigationDelegate = self
        webView.uiDelegate = self
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

    // ---- page -> app bridge

    func userContentController(_ controller: WKUserContentController,
                              didReceive message: WKScriptMessage) {
        guard message.name == "rmath", let body = message.body as? [String: Any],
              let action = body["action"] as? String else { return }
        switch action {
        case "report":
            generateBugReport()
        case "palette":
            if let name = body["name"] as? String { showPalette(named: name) }
        case "restartServer":
            restartServer()
        case "attention":
            // A long task finished while the window was in the background.
            NSApp.requestUserAttention(.informationalRequest)
        case "title":
            if let t = body["title"] as? String { window?.title = t }
        case "screenshot":
            let dest = body["path"] as? String ?? ""
            saveScreenshot(to: dest.isEmpty ? defaultShotPath() : dest)
        default:
            print("rmath bridge: unknown action \(action)")
        }
    }

    // ---- web view navigation: intercept the rmath:// bridge

    func webView(_ webView: WKWebView, decidePolicyFor navigationAction: WKNavigationAction,
                 decisionHandler: @escaping (WKNavigationActionPolicy) -> Void) {
        if let url = navigationAction.request.url, url.scheme == "rmath" {
            switch url.host {
            case "report":
                generateBugReport()
            case "palette":
                let name = url.path.trimmingCharacters(in: CharacterSet(charactersIn: "/"))
                showPalette(named: name.replacingOccurrences(of: "%20", with: " "))
            default:
                break
            }
            decisionHandler(.cancel)
            return
        }
        decisionHandler(.allow)
    }

    // ---- web view UI delegate: alert / confirm / prompt panels

    func webView(_ webView: WKWebView,
                 runJavaScriptAlertPanelWithMessage message: String,
                 initiatedByFrame frame: WKFrameInfo,
                 completionHandler: @escaping () -> Void) {
        let alert = NSAlert()
        alert.messageText = "rMath"
        alert.informativeText = message
        alert.addButton(withTitle: "OK")
        alert.beginSheetModal(for: window) { _ in completionHandler() }
    }

    func webView(_ webView: WKWebView,
                 runJavaScriptConfirmPanelWithMessage message: String,
                 initiatedByFrame frame: WKFrameInfo,
                 completionHandler: @escaping (Bool) -> Void) {
        let alert = NSAlert()
        alert.messageText = "rMath"
        alert.informativeText = message
        alert.addButton(withTitle: "OK")
        alert.addButton(withTitle: "Cancel")
        alert.beginSheetModal(for: window) { resp in
            completionHandler(resp == .alertFirstButtonReturn)
        }
    }

    func webView(_ webView: WKWebView,
                 runJavaScriptTextInputPanelWithPrompt prompt: String,
                 defaultText: String?,
                 initiatedByFrame frame: WKFrameInfo,
                 completionHandler: @escaping (String?) -> Void) {
        let alert = NSAlert()
        alert.messageText = "rMath"
        alert.informativeText = prompt
        alert.addButton(withTitle: "OK")
        alert.addButton(withTitle: "Cancel")
        let field = NSTextField(frame: NSRect(x: 0, y: 0, width: 380, height: 24))
        field.stringValue = defaultText ?? ""
        alert.accessoryView = field
        alert.beginSheetModal(for: window) { resp in
            completionHandler(resp == .alertFirstButtonReturn ? field.stringValue : nil)
        }
    }

    /// Run JavaScript in the notebook page (menu actions route through here).
    func runJS(_ js: String) {
        webView?.evaluateJavaScript(js) { _, err in
            if let err = err { print("menu js error: \(err.localizedDescription)") }
        }
    }

    /// Insert text at the cursor in the notebook input.
    func insert(_ text: String) {
        let escaped = text
            .replacingOccurrences(of: "\\", with: "\\\\")
            .replacingOccurrences(of: "'", with: "\\'")
        runJS("insertText('\(escaped)')")
    }

    @objc func menuEvaluate() { runJS("evalInput()") }
    @objc func menuEvaluateAll() { runJS("evaluateAll()") }
    @objc func menuNew() { runJS("newNotebook()") }
    @objc func menuOpen() { runJS("openNotebook(false)") }
    @objc func menuOpenIntoInput() { runJS("openNotebook(true)") }
    @objc func menuSave() { runJS("saveNotebook()") }
    @objc func menuSavePlot() { runJS("saveLastPlotSvg()") }
    @objc func menuResetKernel() { runJS("resetKernel()") }
    @objc func menuHelp() { runJS("showHelp(true)") }
    @objc func menuAskHarness() { runJS("showAgent('')") }
    @objc func menuLean() { runJS("showLean()") }
    @objc func menuResetPanelLayout() { runJS("resetPanelLayout()") }
    @objc func menuLeanVerify() { runJS("showLean(); setTimeout(leanVerifyLastCell, 300)") }
    @objc func menuFixBug() { runJS("askAboutCell()") }
    @objc func menuStandardForm() { runJS("setForm('standard')") }
    @objc func menuTraditionalForm() { runJS("setForm('traditional')") }
    @objc func menuToggleForm() { runJS("toggleForm()") }
    @objc func menuToggleSteps() { runJS("toggleSteps()") }
    @objc func menuFunctionList() { runJS("showHelp(true)") }
    @objc func menuToggleBrackets() { runJS("toggleBrackets()") }
    @objc func menuFocusInput() { runJS("focusInput()") }
    @objc func menuDeleteAll() { runJS("newNotebook()") }

    // ---- palettes (real floating windows)

    /// Button labels for each palette; the second element is inserted at the
    /// cursor in the notebook (Unicode characters are understood by the kernel).
    static let paletteDefs: [(String, [(String, String)])] = [
        ("Basic Math", [
            ("π", "π"), ("∞", "∞"), ("√", "√"), ("°", "°"),
            ("≤", "≤"), ("≥", "≥"), ("≠", "≠"), ("→", "→"),
            ("×", "×"), ("θ", "θ"), ("α", "α"), ("β", "β"),
            ("γ", "γ"), ("λ", "λ"), ("μ", "μ"), ("σ", "σ"),
        ]),
        ("Algebra", [
            ("Expand[", "Expand["), ("Factor[", "Factor["),
            ("Together[", "Together["), ("Collect[", "Collect["),
            ("Coefficient[", "Coefficient["), ("Exponent[", "Exponent["),
            ("Variables[", "Variables["), ("Solve[", "Solve["),
        ]),
        ("Calculus", [
            ("D[", "D["), ("Integrate[", "Integrate["),
            ("Limit[", "Limit["), ("Series[", "Series["),
            ("Normal[", "Normal["), ("Sum[", "Sum["),
            ("Product[", "Product["), ("N[", "N["),
        ]),
        ("Linear Algebra", [
            ("Det[", "Det["), ("Inverse[", "Inverse["),
            ("Transpose[", "Transpose["), ("Dot[", "Dot["),
            ("LinearSolve[", "LinearSolve["), ("RowReduce[", "RowReduce["),
            ("MatrixPower[", "MatrixPower["), ("IdentityMatrix[", "IdentityMatrix["),
        ]),
        ("Plot", [
            ("Plot[", "Plot["), ("ListPlot[", "ListPlot["),
            ("ListLinePlot[", "ListLinePlot["), ("ParametricPlot[", "ParametricPlot["),
            ("Table[", "Table["), ("Range[", "Range["),
            ("PlotRange →", "PlotRange -> "), ("PlotStyle →", "PlotStyle -> "),
        ]),
        ("Patterns", [
            ("x_", "x_"), ("x__", "x__"), ("x___", "x___"),
            ("_Integer", "_Integer"), ("_?test", "_?test"),
            ("/;", " /; "), ("->", " -> "), (":>", " :> "),
            ("/.", " /. "), ("//.", " //. "),
        ]),
    ]

    var palettes: [String: NSWindow] = [:]

    func paletteView(named name: String, defs: [(String, String)]) -> NSView {
        // Explicit frames: palettes must render identically on screen and in
        // an offscreen snapshot (used for verification).
        let perRow = 4
        let bw: CGFloat = 68, bh: CGFloat = 24, gap: CGFloat = 5, pad: CGFloat = 10
        let rows = (defs.count + perRow - 1) / perRow
        let width = pad * 2 + CGFloat(perRow) * bw + CGFloat(perRow - 1) * gap
        let height = pad * 2 + CGFloat(rows) * bh + CGFloat(max(0, rows - 1)) * gap
        let container = NSView(frame: NSRect(x: 0, y: 0, width: width, height: height))
        container.wantsLayer = true
        for (idx, def) in defs.enumerated() {
            let r = idx / perRow, c = idx % perRow
            let x = pad + CGFloat(c) * (bw + gap)
            let y = height - pad - CGFloat(r + 1) * bh - CGFloat(r) * gap
            let b = PaletteButton(frame: NSRect(x: x, y: y, width: bw, height: bh),
                                  label: def.0, insertText: def.1)
            b.target = self
            b.action = #selector(paletteButtonPressed(_:))
            container.addSubview(b)
        }
        return container
    }

    func showPalette(named name: String) {
        if let existing = palettes[name] {
            existing.makeKeyAndOrderFront(nil)
            NSApp.activate(ignoringOtherApps: true)
            return
        }
        guard let defs = AppDelegate.paletteDefs.first(where: { $0.0 == name })?.1 else { return }
        let view = paletteView(named: name, defs: defs)
        let win = NSWindow(contentRect: view.frame,
                           styleMask: [.titled, .closable, .utilityWindow],
                           backing: .buffered, defer: false)
        win.title = name + " Palette"
        win.isReleasedWhenClosed = false
        win.level = .floating                       // palettes float above the notebook
        win.contentView = view
        win.setFrameAutosaveName("rMathPalette-" + name)
        // park it near the top-right of the main window
        if let main = window {
            let f = main.frame
            win.setFrameOrigin(NSPoint(x: f.maxX - win.frame.width - 20, y: f.maxY - 140))
        } else {
            win.center()
        }
        win.makeKeyAndOrderFront(nil)
        palettes[name] = win
        NSApp.activate(ignoringOtherApps: true)
    }

    @objc func paletteButtonPressed(_ sender: Any?) {
        if let b = sender as? PaletteButton { insert(b.insertText) }
    }

    @objc func openBasicPalette() { showPalette(named: "Basic Math") }
    @objc func openAlgebraPalette() { showPalette(named: "Algebra") }
    @objc func openCalculusPalette() { showPalette(named: "Calculus") }
    @objc func openLinalgPalette() { showPalette(named: "Linear Algebra") }
    @objc func openPlotPalette() { showPalette(named: "Plot") }
    @objc func openPatternPalette() { showPalette(named: "Patterns") }

    // ---- menus

    func buildMenu() {
        let main = NSMenu()

        // --- rMath (application) menu
        let appItem = NSMenuItem(title: "rMath", action: nil, keyEquivalent: "")
        main.addItem(appItem)
        let appMenu = NSMenu(title: "rMath")
        appMenu.addItem(withTitle: "About rMath",
                        action: #selector(NSApplication.orderFrontStandardAboutPanel(_:)),
                        keyEquivalent: "")
        appMenu.addItem(.separator())
        appMenu.addItem(withTitle: "Hide rMath",
                        action: #selector(NSApplication.hide(_:)), keyEquivalent: "h")
        let hideOthers = appMenu.addItem(withTitle: "Hide Others",
                                         action: #selector(NSApplication.hideOtherApplications(_:)),
                                         keyEquivalent: "h")
        hideOthers.keyEquivalentModifierMask = [.command, .option]
        appMenu.addItem(withTitle: "Show All",
                        action: #selector(NSApplication.unhideAllApplications(_:)),
                        keyEquivalent: "")
        appMenu.addItem(.separator())
        appMenu.addItem(withTitle: "Quit rMath",
                        action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        appItem.submenu = appMenu

        // --- File
        let fileItem = NSMenuItem(title: "File", action: nil, keyEquivalent: "")
        main.addItem(fileItem)
        let file = NSMenu(title: "File")
        addItem(file, "New Notebook", #selector(menuNew), "n", target: self)
        addItem(file, "Open…", #selector(menuOpen), "o", target: self)
        addItem(file, "Open into Input…", #selector(menuOpenIntoInput), "o",
                [.command, .shift], target: self)
        addItem(file, "Save Notebook", #selector(menuSave), "s", target: self)
        addItem(file, "Save Last Plot as SVG", #selector(menuSavePlot), "", target: self)
        file.addItem(.separator())
        addItem(file, "Save Screenshot as PNG…", #selector(shootToDesktop), "S",
                [.command, .shift], target: self)
        file.addItem(.separator())
        addItem(file, "Close Window", #selector(NSWindow.performClose(_:)), "w")
        fileItem.submenu = file

        // --- Edit  (standard items use the responder chain: target nil)
        let editItem = NSMenuItem(title: "Edit", action: nil, keyEquivalent: "")
        main.addItem(editItem)
        let edit = NSMenu(title: "Edit")
        addItem(edit, "Undo", Selector(("undo:")), "z")
        addItem(edit, "Redo", Selector(("redo:")), "Z", [.command, .shift])
        edit.addItem(.separator())
        addItem(edit, "Cut", #selector(NSText.cut(_:)), "x")
        addItem(edit, "Copy", #selector(NSText.copy(_:)), "c")
        addItem(edit, "Paste", #selector(NSText.paste(_:)), "v")
        addItem(edit, "Select All", #selector(NSText.selectAll(_:)), "a")
        edit.addItem(.separator())
        addItem(edit, "Delete All Cells", #selector(menuDeleteAll), "k",
                [.command, .shift], target: self)
        editItem.submenu = edit

        // --- Insert
        let insertItem = NSMenuItem(title: "Insert", action: nil, keyEquivalent: "")
        main.addItem(insertItem)
        let insert = NSMenu(title: "Insert")
        let specialItem = addItem(insert, "Special Character", nil, "")
        let specials = NSMenu(title: "Special Character")
        for (label, text) in [("π   Pi", "Pi"), ("∞   Infinity", "Infinity"),
                              ("√   Sqrt", "Sqrt["), ("∫   Integrate", "Integrate["),
                              ("∑   Sum", "Sum["), ("θ   Theta", "Theta"),
                              ("≤   LessEqual", "<= "), ("≥   GreaterEqual", ">= "),
                              ("→   Rule", " -> "), ("≠   Unequal", " != ")] {
            let it = NSMenuItem(title: label,
                                action: #selector(menuInsertSpecial(_:)), keyEquivalent: "")
            it.target = self
            it.representedObject = text
            specials.addItem(it)
        }
        specialItem.submenu = specials
        insert.addItem(.separator())
        addItem(insert, "Plot of Sin[x]", #selector(menuInsertPlot), "", target: self)
        addItem(insert, "Table Example", #selector(menuInsertTable), "", target: self)
        insertItem.submenu = insert

        // --- Evaluation
        let evalItem = NSMenuItem(title: "Evaluation", action: nil, keyEquivalent: "")
        main.addItem(evalItem)
        let evalMenu = NSMenu(title: "Evaluation")
        addItem(evalMenu, "Evaluate Input", #selector(menuEvaluate), "\r", [.shift], target: self)
        addItem(evalMenu, "Evaluate All Cells", #selector(menuEvaluateAll), "\r",
                [.command, .shift], target: self)
        evalMenu.addItem(.separator())
        addItem(evalMenu, "Reset Kernel", #selector(menuResetKernel), "r",
                [.command, .option], target: self)
        evalMenu.addItem(.separator())
        addItem(evalMenu, "Show Steps", #selector(menuToggleSteps), "d",
                [.command, .shift], target: self)
        evalItem.submenu = evalMenu

        // --- Format
        let formatItem = NSMenuItem(title: "Format", action: nil, keyEquivalent: "")
        main.addItem(formatItem)
        let format = NSMenu(title: "Format")
        addItem(format, "StandardForm", #selector(menuStandardForm), "t", target: self)
        addItem(format, "TraditionalForm", #selector(menuTraditionalForm), "t",
                [.command, .shift], target: self)
        addItem(format, "Toggle Typeseing Form", #selector(menuToggleForm), "T",
                [.control, .shift], target: self)
        format.addItem(.separator())
        addItem(format, "Zoom In", #selector(zoomIn), "+", target: self)
        addItem(format, "Zoom Out", #selector(zoomOut), "-", target: self)
        addItem(format, "Actual Size", #selector(zoomReset), "0", target: self)
        formatItem.submenu = format

        // --- Cell
        let cellItem = NSMenuItem(title: "Cell", action: nil, keyEquivalent: "")
        main.addItem(cellItem)
        let cell = NSMenu(title: "Cell")
        addItem(cell, "New Cell (focus input)", #selector(menuFocusInput), "j", target: self)
        addItem(cell, "Show/Hide Cell Brackets", #selector(menuToggleBrackets), "b",
                [.command, .shift], target: self)
        addItem(cell, "Delete All Cells", #selector(menuDeleteAll), "", target: self)
        cellItem.submenu = cell

        // --- Graphics
        let gfxItem = NSMenuItem(title: "Graphics", action: nil, keyEquivalent: "")
        main.addItem(gfxItem)
        let gfx = NSMenu(title: "Graphics")
        addItem(gfx, "Save Last Plot as SVG", #selector(menuSavePlot), "", target: self)
        addItem(gfx, "Plot Sin[x]", #selector(menuInsertPlot), "", target: self)
        gfxItem.submenu = gfx

        // --- Palettes
        let palItem = NSMenuItem(title: "Palettes", action: nil, keyEquivalent: "")
        main.addItem(palItem)
        let pal = NSMenu(title: "Palettes")
        addItem(pal, "Basic Math Palette", #selector(openBasicPalette), "1",
                [.command, .option], target: self)
        addItem(pal, "Algebra Palette", #selector(openAlgebraPalette), "2",
                [.command, .option], target: self)
        addItem(pal, "Calculus Palette", #selector(openCalculusPalette), "3",
                [.command, .option], target: self)
        addItem(pal, "Linear Algebra Palette", #selector(openLinalgPalette), "4",
                [.command, .option], target: self)
        addItem(pal, "Plot Palette", #selector(openPlotPalette), "5",
                [.command, .option], target: self)
        addItem(pal, "Patterns Palette", #selector(openPatternPalette), "6",
                [.command, .option], target: self)
        palItem.submenu = pal

        // --- Window
        let winItem = NSMenuItem(title: "Window", action: nil, keyEquivalent: "")
        main.addItem(winItem)
        let win = NSMenu(title: "Window")
        addItem(win, "Minimize", #selector(NSWindow.performMiniaturize(_:)), "m")
        addItem(win, "Zoom", #selector(NSWindow.performZoom(_:)), "")
        win.addItem(.separator())
        addItem(win, "Bring All to Front", #selector(NSApplication.arrangeInFront(_:)), "")
        winItem.submenu = win
        NSApp.windowsMenu = win

        // --- Help  (macOS routes the last menu to the Help menu automatically)
        let helpItem = NSMenuItem(title: "Help", action: nil, keyEquivalent: "")
        main.addItem(helpItem)
        let help = NSMenu(title: "Help")
        addItem(help, "rMath Help", #selector(menuHelp), "?", target: self)
        addItem(help, "Ask the Harness…", #selector(menuAskHarness), "H",
                [.command, .shift], target: self)
        addItem(help, "Lean…", #selector(menuLean), "L",
                [.command, .shift], target: self)
        addItem(help, "Verify in Lean", #selector(menuLeanVerify), "V",
                [.command, .option], target: self)
        addItem(help, "Reset Panel Layout", #selector(menuResetPanelLayout), "",
                [], target: self)
        addItem(help, "Report a Bug…", #selector(menuReportBug), "B",
                [.command, .shift], target: self)
        addItem(help, "Function List", #selector(menuFunctionList), "", target: self)
        helpItem.submenu = help
        NSApp.helpMenu = help

        NSApp.mainMenu = main
    }

    /// Helper: append a menu item. Standard selectors (cut:, copy:, …) must
    /// keep a nil target so the responder chain handles them; our own actions
    /// pass target: self.
    @discardableResult
    func addItem(_ menu: NSMenu, _ title: String, _ action: Selector?,
                 _ key: String, _ mods: NSEvent.ModifierFlags = [.command],
                 target: AnyObject? = nil) -> NSMenuItem {
        let it = NSMenuItem(title: title, action: action, keyEquivalent: key)
        it.target = target
        if !key.isEmpty { it.keyEquivalentModifierMask = mods }
        if action == nil { it.isEnabled = false }
        menu.addItem(it)
        return it
    }

    @objc func menuInsertSpecial(_ sender: NSMenuItem) {
        if let text = sender.representedObject as? String { insert(text) }
    }
    @objc func menuInsertPlot() { insert("Plot[Sin[x], {x, 0, 2 Pi}]") }
    @objc func menuInsertTable() { insert("Table[i^2, {i, 1, 10}]") }

    @objc func reloadPage() { webView.reload() }
    @objc func zoomReset() { setZoom(1.0) }
    @objc func zoomIn() { setZoom(min(3.0, webView.pageZoom + 0.1)) }
    @objc func zoomOut() { setZoom(max(0.4, webView.pageZoom - 0.1)) }

    func setZoom(_ z: Double) {
        webView.pageZoom = z
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

    /// Watch the server this app owns and bring it back if it dies.
    ///
    /// Without this, killing the helper (a crash, or reinstalling the app while
    /// a window is open) left the page making requests to a dead port, where
    /// every fetch fails with WebKit's bare "Load failed".
    func startServerWatchdog() {
        watchTimer?.invalidate()
        var consecutiveFailures = 0
        watchTimer = Timer.scheduledTimer(withTimeInterval: 2.0, repeats: true) { [weak self] _ in
            guard let self = self, self.ownsServer else { return }
            let died = self.server.map { !$0.isRunning } ?? true
            if healthOK(self.port) {
                consecutiveFailures = 0
                return
            }
            consecutiveFailures += 1
            guard died || consecutiveFailures >= 2 else { return }
            self.appLog("rMath: notebook server is not responding — restarting it")
            consecutiveFailures = 0
            self.restartServer()
        }
    }

    /// Stop the helper and start it again, then reload the page onto it.
    func restartServer(completion: (() -> Void)? = nil) {
        guard !restarting else { return }
        restarting = true
        stopServer()
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.4) { [weak self] in
            guard let self = self else { return }
            self.startServer { ok in
                self.restarting = false
                // Deliberately *no* page reload: the notebook lives in the page,
                // so reloading would throw away the user's cells. The page's own
                // health poll notices the recovery and says so.
                if !ok {
                    self.appLog("rMath: could not restart the notebook server")
                }
                completion?()
            }
        }
    }

    /// Append a line to the app log (the same file the notebook server writes).
    func appLog(_ line: String) {
        let url = logURL()
        if let h = serverLog {
            h.write(Data((line + "\n").utf8))
            return
        }
        if !FileManager.default.fileExists(atPath: url.path) {
            FileManager.default.createFile(atPath: url.path, contents: nil)
        }
        if let h = try? FileHandle(forWritingTo: url) {
            h.seekToEndOfFile()
            h.write(Data((line + "\n").utf8))
            try? h.close()
        }
    }

    func stopServer() {
        watchTimer?.invalidate()
        watchTimer = nil
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
