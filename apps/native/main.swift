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
    @objc func menuStandardForm() { runJS("setForm('standard')") }
    @objc func menuTraditionalForm() { runJS("setForm('traditional')") }
    @objc func menuToggleForm() { runJS("toggleForm()") }
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
    ///   line 4: optional target — "page" (default) or "palette:<name>"
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
        let what = lines.isEmpty ? "page" : lines.removeFirst().trimmingCharacters(in: .whitespaces)
        let target = dest.isEmpty ? defaultShotPath() : dest

        if what == "report" {
            generateBugReport(note: "triggered from the command line") { path in
                self.finishShot(path.map { "ok \($0)" } ?? "err report failed")
            }
            return
        }

        if what.hasPrefix("palette:") {
            let name = String(what.dropFirst("palette:".count))
            showPalette(named: name)
            DispatchQueue.main.asyncAfter(deadline: .now() + max(delay, 1.0)) {
                self.savePaletteScreenshot(named: name, to: target)
            }
            return
        }
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

    /// Snapshot a palette window by rendering its layer tree (works for an
    /// on-screen window and needs no screen-recording permission).
    func savePaletteScreenshot(named name: String, to path: String) {
        guard let win = palettes[name], let view = win.contentView else {
            finishShot("err no palette \(name)")
            return
        }
        let dir = URL(fileURLWithPath: path).deletingLastPathComponent()
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        view.wantsLayer = true
        guard let layer = view.layer else {
            finishShot("err palette has no layer")
            return
        }
        let w = Int(view.bounds.width), h = Int(view.bounds.height)
        guard w > 0, h > 0,
              let rep = NSBitmapImageRep(
                bitmapDataPlanes: nil, pixelsWide: w, pixelsHigh: h,
                bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0) else {
            finishShot("err palette bitmap")
            return
        }
        rep.size = NSSize(width: view.bounds.width, height: view.bounds.height)
        NSGraphicsContext.saveGraphicsState()
        if let ctx = NSGraphicsContext(bitmapImageRep: rep) {
            NSGraphicsContext.current = ctx
            layer.render(in: ctx.cgContext)
        }
        NSGraphicsContext.restoreGraphicsState()
        if let png = rep.representation(using: .png, properties: [:]) {
            try? png.write(to: URL(fileURLWithPath: path))
            finishShot("ok \(path)")
        } else {
            finishShot("err png encode")
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

    /// Snapshot the page to PNG data (no side effects).
    func snapshotPNG(completion: @escaping (Data?) -> Void) {
        guard let web = webView else { completion(nil); return }
        let cfg = WKSnapshotConfiguration()
        cfg.rect = web.bounds
        web.takeSnapshot(with: cfg) { image, _ in
            guard let image = image,
                  let tiff = image.tiffRepresentation,
                  let rep = NSBitmapImageRep(data: tiff) else { completion(nil); return }
            completion(rep.representation(using: .png, properties: [:]))
        }
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
            // Ask the page for its full height so the PDF covers all the
            // content rather than just the visible viewport.
            web.evaluateJavaScript(
                "Math.max(document.body.scrollHeight, document.documentElement.scrollHeight)"
            ) { value, _ in
                var height = web.bounds.height
                if let n = value as? NSNumber, n.doubleValue > 0 {
                    height = CGFloat(n.doubleValue)
                }
                let cfg = WKPDFConfiguration()
                cfg.rect = CGRect(x: 0, y: 0, width: web.bounds.width, height: height)
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

    /// Build a self-contained bug report: screenshot, page transcript, the
    /// last error, versions, and the tail of the log. The path and a one-line
    /// summary go on the clipboard, ready to paste into a chat.
    func generateBugReport(note: String = "", completion: ((String?) -> Void)? = nil) {
        let stamp = timestamp()
        let dir = reportDir()
        let png = dir.appendingPathComponent("bug-\(stamp).png")
        let md = dir.appendingPathComponent("bug-\(stamp).md")

        runJSCollect("reportContext()") { ctx in
            self.snapshotPNG { pngData in
                if let pngData = pngData {
                    try? pngData.write(to: png)
                }
                let body = self.buildReportBody(stamp: stamp, png: png.path,
                                                context: ctx, note: note)
                try? body.write(to: md, atomically: true, encoding: .utf8)

                // Paste-ready text on the clipboard.
                var paste = "rMath bug report: \(md.path)"
                if pngData != nil { paste += "\nscreenshot: \(png.path)" }
                if let summary = self.reportSummary(ctx) { paste += "\n" + summary }
                if !note.isEmpty { paste += "\nnote: " + note }
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(paste, forType: .string)

                self.runJS("showReport('\(md.path.replacingOccurrences(of: "'", with: "\\'"))', '\(self.reportSummary(ctx)?.replacingOccurrences(of: "'", with: "\\'") ?? "")')")
                completion?(md.path)
            }
        }
    }

    func reportSummary(_ ctx: String?) -> String? {
        guard let ctx = ctx, let data = ctx.data(using: .utf8),
              let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else { return nil }
        guard let last = obj["lastError"] as? [String: Any] else { return "no errors in this session" }
        let input = (last["input"] as? String ?? "").prefix(120)
        let msg = ((last["error"] as? String ?? "") + " " + (last["messages"] as? String ?? ""))
            .trimmingCharacters(in: .whitespacesAndNewlines).prefix(160)
        return "last error: \(input)\(msg.isEmpty ? "" : " — " + msg)"
    }

    func buildReportBody(stamp: String, png: String, context: String?, note: String) -> String {
        var out = "# rMath bug report — \(stamp)\n\n"
        out += "- app: rMath \(Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "?")\n"
        out += "- bundle: \(Bundle.main.bundlePath)\n"
        out += "- kernel: \(kernelVersion())\n"
        out += "- python: \(pythonVersion())\n"
        out += "- port: \(port)\n"
        out += "- log: \(logURL().path)\n"
        out += "- screenshot: \(png)\n"
        if !note.isEmpty { out += "- note: \(note)\n" }
        out += "\n## Page context\n\n```json\n\(context ?? "(unavailable)")\n```\n"
        out += "\n## Log tail\n\n```\n\(logTail(40))\n```\n"
        return out
    }

    /// Run JS and pass its string result back on the main thread.
    func runJSCollect(_ js: String, completion: @escaping (String?) -> Void) {
        webView?.evaluateJavaScript(js) { value, err in
            if let err = err { print("js collect error: \(err.localizedDescription)") }
            completion(value as? String)
        }
    }

    func kernelVersion() -> String {
        let bin = resourcesDir().appendingPathComponent("rmath")
        let p = Process()
        p.executableURL = bin
        p.arguments = ["--version"]
        let pipe = Pipe()
        p.standardOutput = pipe
        try? p.run()
        p.waitUntilExit()
        let data = pipe.fileHandleForReading.readDataToEndOfFile()
        return String(data: data, encoding: .utf8)?.trimmingCharacters(in: .whitespacesAndNewlines)
            ?? "unknown"
    }

    func pythonVersion() -> String {
        guard let py = pythonCandidates().first(where: usablePython) else { return "not found" }
        let p = Process()
        p.executableURL = URL(fileURLWithPath: py)
        p.arguments = ["-c", "import sys; print(sys.executable, sys.version.split()[0])"]
        let pipe = Pipe()
        p.standardOutput = pipe
        try? p.run()
        p.waitUntilExit()
        let data = pipe.fileHandleForReading.readDataToEndOfFile()
        let text = String(data: data, encoding: .utf8)?
            .trimmingCharacters(in: .whitespacesAndNewlines) ?? "?"
        return "\(text)  [\(py)]"
    }

    func logTail(_ n: Int) -> String {
        guard let text = try? String(contentsOf: logURL(), encoding: .utf8) else {
            return "(no log)"
        }
        let lines = text.split(separator: "\n", omittingEmptySubsequences: false)
        return lines.suffix(n).joined(separator: "\n")
    }

    @objc func menuReportBug() {
        generateBugReport { path in
            if let path = path { print("report: \(path)") }
        }
    }

    @objc func shootToDesktop() {
        saveScreenshot(to: defaultShotPath())
    }

    /// Ask the running instance to generate a bug report.
    func requestReport() -> Bool {
        if !healthOK(port) {
            FileHandle.standardError.write(Data(
                "rMath: no running instance on port \(port)\n".utf8))
            return false
        }
        try? FileManager.default.removeItem(at: shotDoneURL())
        let request = "\n\n0.5\nreport\n"
        do {
            try request.write(to: shotRequestURL(), atomically: true, encoding: .utf8)
        } catch { return false }
        let deadline = Date().addingTimeInterval(30)
        while Date() < deadline {
            if let done = try? String(contentsOf: shotDoneURL(), encoding: .utf8) {
                try? FileManager.default.removeItem(at: shotDoneURL())
                print(done)
                return done.hasPrefix("ok ")
            }
            usleep(150_000)
        }
        return false
    }

    /// Ask the running instance for a screenshot; returns true on success.
    func requestScreenshot(dest: String, js: String = "", delay: Double = 2.5,
                           what: String = "page") -> Bool {
        if !healthOK(port) {
            FileHandle.standardError.write(Data(
                "rMath: no running instance on port \(port) to screenshot\n".utf8))
            return false
        }
        try? FileManager.default.removeItem(at: shotDoneURL())
        let target = dest.isEmpty ? URL(fileURLWithPath: NSTemporaryDirectory())
            .appendingPathComponent("rMath-\(timestamp()).png").path : dest
        let request = "\(target)\n\(js.replacingOccurrences(of: "\n", with: " "))\n\(delay)\n\(what)\n"
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
