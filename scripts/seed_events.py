#!/usr/bin/env python3
"""Insert seed events with realistic stack traces into soika.db."""

import json
import os
import sqlite3
import sys
from datetime import datetime, timedelta, timezone

DB_FILE = os.environ.get("DATABASE_URL", "sqlite://soika.db").removeprefix("sqlite://")

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def uid(n: int, group: int = 0) -> str:
    return f"e{group:07d}-0000-0000-0000-{n:012d}"

def hex32(n: int, group: int = 0) -> str:
    return f"{group:08x}{n:024x}"

def ts(base: str, offset_hours: float = 0) -> str:
    dt = datetime.fromisoformat(base.replace("Z", "+00:00"))
    dt = dt + timedelta(hours=offset_hours)
    return dt.strftime("%Y-%m-%dT%H:%M:%SZ")


# ---------------------------------------------------------------------------
# Stack-trace templates
# ---------------------------------------------------------------------------

def frames_kotlin(*pairs):
    """pairs: (module, function, filename, lineno, in_app, context_line)"""
    result = []
    for m, fn, file, line, in_app, ctx in pairs:
        f = {"module": m, "function": fn, "filename": file, "lineno": line, "in_app": in_app}
        if ctx:
            f["context_line"] = ctx
        result.append(f)
    return result

def frames_go(*pairs):
    result = []
    for m, fn, file, line, in_app, ctx in pairs:
        f = {"module": m, "function": fn, "abs_path": file, "lineno": line, "in_app": in_app}
        if ctx:
            f["context_line"] = ctx
        result.append(f)
    return result

def frames_swift(*pairs):
    result = []
    for m, fn, file, line, in_app, ctx in pairs:
        f = {"module": m, "function": fn, "filename": file, "lineno": line, "in_app": in_app}
        if ctx:
            f["context_line"] = ctx
        result.append(f)
    return result


# ---------------------------------------------------------------------------
# Event specs: (issue_id, first_seen, level, env, release, exc_type, exc_value, frames)
# ---------------------------------------------------------------------------

# issue_id prefix → project kind
# c0000001 = Deep Reader  (Kotlin/Java)
# c0000002 = API Gateway  (Go)
# c0000003 = iOS App      (Swift)
# c0000004 = Android App  (Kotlin)

EVENTS = [

    # ── Deep Reader ──────────────────────────────────────────────────────────

    # 001 NullPointerException in BookParser.parse()
    {
        "issue_id":  "c0000001-0000-0000-0000-000000000001",
        "project_id":"ddc73435-a0fe-42d6-a63e-6a0d1f4c220a",
        "ts":        "2026-06-08T14:30:00Z",
        "level":     "error",
        "env":       "production",
        "release":   "2.3.1",
        "exc_type":  "java.lang.NullPointerException",
        "exc_value": "Attempt to invoke virtual method on a null object reference",
        "frames": frames_kotlin(
            ("java.lang.reflect",       "invoke",           "Method.java",          511, False, None),
            ("android.app",             "callActivityOnCreate","Activity.java",      8079, False, None),
            ("com.deepreader.io",        "parse",            "BookParser.java",      142, True,  "    return metadata.getTitle().trim();"),
            ("com.deepreader.library",   "loadBook",         "LibraryManager.java",  88,  True,  "    BookParser parser = getParser(format);"),
            ("com.deepreader.ui",        "onFileSelected",   "ReaderActivity.java",  234, True,  "    libraryManager.loadBook(uri);"),
        ),
    },

    # 002 Failed to load EPUB metadata
    {
        "issue_id":  "c0000001-0000-0000-0000-000000000002",
        "project_id":"ddc73435-a0fe-42d6-a63e-6a0d1f4c220a",
        "ts":        "2026-06-07T11:00:00Z",
        "level":     "error",
        "env":       "production",
        "release":   "2.3.1",
        "exc_type":  "com.deepreader.epub.EpubParseException",
        "exc_value": "Missing required OPF file in EPUB container",
        "frames": frames_kotlin(
            ("java.util.zip",            "ZipFile.getEntry",  "ZipFile.java",         189, False, None),
            ("com.deepreader.epub",      "findOpf",           "EpubLoader.kt",        55,  True,  "    val entry = zip.getEntry(\"content.opf\") ?: throw EpubParseException(\"Missing OPF\")"),
            ("com.deepreader.epub",      "loadMetadata",      "EpubLoader.kt",        88,  True,  "    val opf = findOpf(zip)"),
            ("com.deepreader.ui",        "openDocument",      "ReaderViewModel.kt",   67,  True,  "    loader.loadMetadata(file)"),
        ),
    },

    # 003 Sync conflict
    {
        "issue_id":  "c0000001-0000-0000-0000-000000000003",
        "project_id":"ddc73435-a0fe-42d6-a63e-6a0d1f4c220a",
        "ts":        "2026-06-09T07:00:00Z",
        "level":     "warning",
        "env":       "production",
        "release":   "2.3.2",
        "exc_type":  "com.deepreader.sync.ConflictException",
        "exc_value": "Local revision 47 conflicts with remote revision 52",
        "frames": frames_kotlin(
            ("com.deepreader.sync",      "resolveConflict",   "SyncService.kt",       211, True,  "    throw ConflictException(\"Local revision $local conflicts with remote revision $remote\")"),
            ("com.deepreader.sync",      "sync",              "SyncService.kt",       178, True,  "    resolveConflict(local, remote)"),
            ("androidx.work",            "doWork",            "Worker.java",          81,  False, None),
        ),
    },

    # 005 SQLiteDatabaseLockedException on notes save
    {
        "issue_id":  "c0000001-0000-0000-0000-000000000005",
        "project_id":"ddc73435-a0fe-42d6-a63e-6a0d1f4c220a",
        "ts":        "2026-06-09T06:45:00Z",
        "level":     "error",
        "env":       "production",
        "release":   "2.3.2",
        "exc_type":  "android.database.sqlite.SQLiteDatabaseLockedException",
        "exc_value": "database is locked (code 5): while compiling: INSERT INTO notes ...",
        "frames": frames_kotlin(
            ("android.database.sqlite",  "nativeExecute",     "SQLiteConnection.java", 582, False, None),
            ("android.database.sqlite",  "execute",           "SQLiteConnection.java", 905, False, None),
            ("com.deepreader.db",        "saveNote",          "NotesRepository.kt",   305, True,  "    db.execSQL(\"INSERT INTO notes (id, content) VALUES (?, ?)\", arrayOf(id, content))"),
            ("com.deepreader.ui",        "onSaveClicked",     "NoteEditorFragment.kt", 88, True,  "    repository.saveNote(currentNote)"),
        ),
    },

    # 007 Corrupt highlight index
    {
        "issue_id":  "c0000001-0000-0000-0000-000000000007",
        "project_id":"ddc73435-a0fe-42d6-a63e-6a0d1f4c220a",
        "ts":        "2026-06-08T18:00:00Z",
        "level":     "error",
        "env":       "staging",
        "release":   "2.3.2",
        "exc_type":  "com.deepreader.migrate.MigrationException",
        "exc_value": "Highlight index version mismatch: expected v3, found v1",
        "frames": frames_kotlin(
            ("com.deepreader.migrate",   "validateIndex",     "HighlightMigration.kt", 44, True,  "    check(index.version == TARGET_VERSION) { \"Highlight index version mismatch\" }"),
            ("com.deepreader.migrate",   "migrate",           "HighlightMigration.kt", 29, True,  "    validateIndex(db)"),
            ("com.deepreader.db",        "onUpgrade",         "AppDatabase.kt",        67, True,  "    HighlightMigration().migrate(db)"),
        ),
    },

    # 008 OOM crash when opening >500-page PDF
    {
        "issue_id":  "c0000001-0000-0000-0000-000000000008",
        "project_id":"ddc73435-a0fe-42d6-a63e-6a0d1f4c220a",
        "ts":        "2026-06-09T05:30:00Z",
        "level":     "fatal",
        "env":       "production",
        "release":   "2.3.2",
        "exc_type":  "java.lang.OutOfMemoryError",
        "exc_value": "Failed to allocate a 48184320 byte allocation with 16776960 free bytes and 38MB until OOM",
        "frames": frames_kotlin(
            ("android.graphics",         "BitmapFactory.decodeStream", "BitmapFactory.java", 602, False, None),
            ("com.deepreader.pdf",       "renderPage",        "PdfLoader.kt",         77,  True,  "    val bmp = BitmapFactory.decodeStream(pageStream, null, opts)"),
            ("com.deepreader.pdf",       "preloadPages",      "PdfLoader.kt",         112, True,  "    pages.forEach { renderPage(it) }"),
            ("com.deepreader.ui",        "onDocumentOpened",  "PdfReaderActivity.kt", 55,  True,  "    loader.preloadPages(doc)"),
        ),
    },

    # 009 Search index rebuild fails
    {
        "issue_id":  "c0000001-0000-0000-0000-000000000009",
        "project_id":"ddc73435-a0fe-42d6-a63e-6a0d1f4c220a",
        "ts":        "2026-06-09T08:00:00Z",
        "level":     "error",
        "env":       "production",
        "release":   "2.3.2",
        "exc_type":  "com.deepreader.search.IndexException",
        "exc_value": "Lucene index corrupt: checksum mismatch at segment _3",
        "frames": frames_kotlin(
            ("org.apache.lucene",        "checkIndex",        "CheckIndex.java",      301, False, None),
            ("com.deepreader.search",    "rebuildIndex",      "SearchIndexer.kt",     221, True,  "    lucene.checkIndex(dir)"),
            ("com.deepreader.search",    "scheduleRebuild",   "SearchIndexer.kt",     189, True,  "    rebuildIndex(library.books)"),
        ),
    },

    # 012 Crash on rotating device
    {
        "issue_id":  "c0000001-0000-0000-0000-000000000012",
        "project_id":"ddc73435-a0fe-42d6-a63e-6a0d1f4c220a",
        "ts":        "2026-06-09T09:15:00Z",
        "level":     "error",
        "env":       "production",
        "release":   "2.3.2",
        "exc_type":  "java.lang.IllegalStateException",
        "exc_value": "Can not perform this action after onSaveInstanceState",
        "frames": frames_kotlin(
            ("androidx.fragment.app",    "checkStateLoss",    "FragmentManager.java", 1536, False, None),
            ("androidx.fragment.app",    "enqueueAction",     "FragmentManager.java", 1515, False, None),
            ("com.deepreader.ui",        "onConfigChanged",   "ReadingActivity.kt",   156, True,  "    supportFragmentManager.beginTransaction().replace(R.id.container, fragment).commit()"),
        ),
    },

    # 013 BookmarkSync duplicate entries
    {
        "issue_id":  "c0000001-0000-0000-0000-000000000013",
        "project_id":"ddc73435-a0fe-42d6-a63e-6a0d1f4c220a",
        "ts":        "2026-06-09T10:00:00Z",
        "level":     "warning",
        "env":       "production",
        "release":   "2.3.2",
        "exc_type":  "com.deepreader.sync.DuplicateEntryException",
        "exc_value": "Bookmark id=b-4421 already exists for book isbn=978-3-16-148410-0",
        "frames": frames_kotlin(
            ("com.deepreader.sync",      "mergeBookmarks",    "BookmarkSync.kt",      67, True, "    if (existing.contains(bm.id)) throw DuplicateEntryException(\"Bookmark id=${bm.id} already exists\")"),
            ("com.deepreader.sync",      "onSyncComplete",    "BookmarkSync.kt",      45, True, "    mergeBookmarks(remote)"),
        ),
    },

    # ── API Gateway ───────────────────────────────────────────────────────────

    # 001 Timeout connecting to auth service
    {
        "issue_id":  "c0000002-0000-0000-0000-000000000001",
        "project_id":"b1b2c3d4-0001-0001-0001-000000000001",
        "ts":        "2026-06-09T11:00:00Z",
        "level":     "error",
        "env":       "production",
        "release":   "v1.8.0",
        "exc_type":  "context.DeadlineExceeded",
        "exc_value": "context deadline exceeded (auth-service: tcp dial timeout after 5s)",
        "frames": frames_go(
            ("net",                      "(*netFD).connect",         "net/fd_unix.go",           588, False, None),
            ("net/http",                 "(*Transport).dialConn",    "net/http/transport.go",    1750, False, None),
            ("github.com/tinyops/soika/internal/auth",
                                         "(*Client).Verify",         "internal/auth/client.go",  88,  True,  "\tctx, cancel := context.WithTimeout(ctx, 5*time.Second)"),
            ("github.com/tinyops/soika/middleware",
                                         "AuthMiddleware",           "middleware/auth.go",        88,  True,  "\tclaims, err := authClient.Verify(r.Context(), token)"),
            ("net/http",                 "(*ServeMux).ServeHTTP",    "net/http/server.go",       2534, False, None),
        ),
    },

    # 002 Panic: nil pointer dereference in rate limiter
    {
        "issue_id":  "c0000002-0000-0000-0000-000000000002",
        "project_id":"b1b2c3d4-0001-0001-0001-000000000001",
        "ts":        "2026-06-09T10:30:00Z",
        "level":     "fatal",
        "env":       "production",
        "release":   "v1.8.1",
        "exc_type":  "panic",
        "exc_value": "runtime error: invalid memory address or nil pointer dereference",
        "frames": frames_go(
            ("runtime",                  "gopanic",                  "runtime/panic.go",         914, False, None),
            ("runtime",                  "panicmem",                 "runtime/signal_unix.go",   875, False, None),
            ("github.com/tinyops/soika/ratelimit",
                                         "(*Limiter).Allow",         "ratelimit/limiter.go",      54, True,  "\treturn l.store.Incr(key)  // l.store is nil after failed init"),
            ("github.com/tinyops/soika/middleware",
                                         "RateLimit",                "middleware/ratelimit.go",   33, True,  "\tif !limiter.Allow(ip) {"),
        ),
    },

    # 005 Request body too large
    {
        "issue_id":  "c0000002-0000-0000-0000-000000000005",
        "project_id":"b1b2c3d4-0001-0001-0001-000000000001",
        "ts":        "2026-06-08T19:00:00Z",
        "level":     "warning",
        "env":       "production",
        "release":   "v1.8.0",
        "exc_type":  "http.MaxBytesError",
        "exc_value": "http: request body too large (limit: 1048576 bytes)",
        "frames": frames_go(
            ("net/http",                 "(*maxBytesReader).Read",   "net/http/request.go",      1134, False, None),
            ("encoding/json",            "(*Decoder).Decode",        "encoding/json/stream.go",  74,   False, None),
            ("github.com/tinyops/soika/router",
                                         "parseBody",                "router/middleware.go",      77,  True,  "\tif err := json.NewDecoder(http.MaxBytesReader(w, r.Body, 1<<20)).Decode(&req); err != nil {"),
            ("github.com/tinyops/soika/handlers",
                                         "handleWebhook",            "handlers/webhook.go",       55,  True,  "\tbody, err := parseBody(r)"),
        ),
    },

    # 006 Upstream 503: payment service
    {
        "issue_id":  "c0000002-0000-0000-0000-000000000006",
        "project_id":"b1b2c3d4-0001-0001-0001-000000000001",
        "ts":        "2026-06-09T12:00:00Z",
        "level":     "error",
        "env":       "production",
        "release":   "v1.8.1",
        "exc_type":  "proxy.UpstreamError",
        "exc_value": "upstream payment-service returned 503 Service Unavailable",
        "frames": frames_go(
            ("net/http",                 "(*Client).Do",             "net/http/client.go",       725,  False, None),
            ("github.com/tinyops/soika/proxy",
                                         "(*Upstream).Forward",      "proxy/upstream.go",         201, True,  "\tresp, err := http.DefaultClient.Do(req)"),
            ("github.com/tinyops/soika/proxy",
                                         "handlePayment",            "proxy/upstream.go",         155, True,  "\treturn upstream.Forward(ctx, r)"),
            ("github.com/tinyops/soika/handlers",
                                         "PaymentProxy",             "handlers/payment.go",        88, True,  "\tif err := proxy.handlePayment(ctx, w, r); err != nil {"),
        ),
    },

    # 008 gRPC stream closed unexpectedly
    {
        "issue_id":  "c0000002-0000-0000-0000-000000000008",
        "project_id":"b1b2c3d4-0001-0001-0001-000000000001",
        "ts":        "2026-06-09T09:00:00Z",
        "level":     "error",
        "env":       "staging",
        "release":   "v1.8.1",
        "exc_type":  "io.EOF",
        "exc_value": "EOF: server closed the stream unexpectedly",
        "frames": frames_go(
            ("google.golang.org/grpc/internal/transport",
                                         "(*http2Client).reader",    "transport/http2_client.go", 1534, False, None),
            ("google.golang.org/grpc",   "(*ClientStream).RecvMsg",  "stream.go",                 946, False, None),
            ("github.com/tinyops/soika/grpc",
                                         "(*StreamClient).Recv",     "grpc/stream.go",             99, True,  "\tif err := stream.RecvMsg(resp); err != nil {"),
            ("github.com/tinyops/soika/handlers",
                                         "StreamEvents",             "handlers/stream.go",         44, True,  "\tfor client.Recv(ctx, &ev) == nil {"),
        ),
    },

    # 010 Context deadline exceeded: DB query
    {
        "issue_id":  "c0000002-0000-0000-0000-000000000010",
        "project_id":"b1b2c3d4-0001-0001-0001-000000000001",
        "ts":        "2026-06-09T13:00:00Z",
        "level":     "error",
        "env":       "production",
        "release":   "v1.8.1",
        "exc_type":  "context.DeadlineExceeded",
        "exc_value": "context deadline exceeded (postgres query exceeded 10s timeout)",
        "frames": frames_go(
            ("database/sql",             "(*DB).QueryContext",        "database/sql/sql.go",      1645, False, None),
            ("github.com/tinyops/soika/db",
                                         "(*Pool).Query",             "db/pool.go",                67,  True,  "\trows, err := p.db.QueryContext(ctx, query, args...)"),
            ("github.com/tinyops/soika/repo",
                                         "(*EventRepo).FindByProject","repo/events.go",            88,  True,  "\treturn pool.Query(ctx, selectEventsByProject, projectID)"),
            ("github.com/tinyops/soika/handlers",
                                         "listEvents",               "handlers/events.go",         55,  True,  "\tevents, err := repo.FindByProject(ctx, projectID)"),
        ),
    },

    # 011 CORS preflight rejected
    {
        "issue_id":  "c0000002-0000-0000-0000-000000000011",
        "project_id":"b1b2c3d4-0001-0001-0001-000000000001",
        "ts":        "2026-06-08T21:00:00Z",
        "level":     "warning",
        "env":       "production",
        "release":   "v1.8.0",
        "exc_type":  "cors.OriginNotAllowed",
        "exc_value": "CORS: origin https://app.partner.io not in allowlist",
        "frames": frames_go(
            ("github.com/rs/cors",       "(*Cors).handlePreflight",  "cors/cors.go",             250, False, None),
            ("github.com/tinyops/soika/cors",
                                         "(*Handler).check",         "cors/handler.go",           33, True,  "\tif !c.isOriginAllowed(origin) { return cors.OriginNotAllowed }"),
            ("net/http",                 "(*ServeMux).ServeHTTP",    "net/http/server.go",        2534, False, None),
        ),
    },

    # 013 Retry budget exhausted
    {
        "issue_id":  "c0000002-0000-0000-0000-000000000013",
        "project_id":"b1b2c3d4-0001-0001-0001-000000000001",
        "ts":        "2026-06-09T14:00:00Z",
        "level":     "error",
        "env":       "production",
        "release":   "v1.8.1",
        "exc_type":  "retry.BudgetExhausted",
        "exc_value": "retry budget exhausted after 3 attempts: last error: connection refused",
        "frames": frames_go(
            ("github.com/tinyops/soika/retry",
                                         "(*Policy).Do",             "retry/policy.go",           80, True,  "\treturn retry.BudgetExhausted{Attempts: p.max, Last: lastErr}"),
            ("github.com/tinyops/soika/handlers",
                                         "callUpstream",             "handlers/upstream.go",      44, True,  "\tif err := retryPolicy.Do(ctx, fn); err != nil {"),
        ),
    },

    # 015 Memory leak: goroutine not released
    {
        "issue_id":  "c0000002-0000-0000-0000-000000000015",
        "project_id":"b1b2c3d4-0001-0001-0001-000000000001",
        "ts":        "2026-06-09T08:00:00Z",
        "level":     "warning",
        "env":       "production",
        "release":   "v1.8.0",
        "exc_type":  "goroutine.Leak",
        "exc_value": "goroutine leaked: stream handler goroutine still running 60s after request cancel",
        "frames": frames_go(
            ("github.com/tinyops/soika/handlers",
                                         "StreamEvents.func1",       "handlers/stream.go",        44, True, "\tfor {\n\t\tselect {\n\t\tcase ev := <-ch:\n\t\t\t// ctx never checked"),
        ),
    },

    # ── iOS App ───────────────────────────────────────────────────────────────

    # 001 EXC_BAD_ACCESS in UICollectionView reloadData
    {
        "issue_id":  "c0000003-0000-0000-0000-000000000001",
        "project_id":"b1b2c3d4-0002-0002-0002-000000000002",
        "ts":        "2026-06-09T11:30:00Z",
        "level":     "fatal",
        "env":       "production",
        "release":   "4.1.0",
        "exc_type":  "EXC_BAD_ACCESS",
        "exc_value": "SIGSEGV at 0x0000000000000010 — likely null pointer dereference in UICollectionView",
        "frames": frames_swift(
            ("UIKitCore",                "-[UICollectionView _updateVisibleCellsNow:]", "UICollectionView.m", 4521, False, None),
            ("UIKitCore",                "-[UICollectionView reloadData]",              "UICollectionView.m", 1882, False, None),
            ("DeepReaderApp",            "applySnapshot",               "FeedViewController.swift", 203, True,  "        collectionView.reloadData()"),
            ("DeepReaderApp",            "viewModel(_:didUpdateFeed:)", "FeedViewController.swift", 178, True,  "        applySnapshot(feed)"),
        ),
    },

    # 002 KeychainError: errSecDuplicateItem
    {
        "issue_id":  "c0000003-0000-0000-0000-000000000002",
        "project_id":"b1b2c3d4-0002-0002-0002-000000000002",
        "ts":        "2026-06-09T09:00:00Z",
        "level":     "error",
        "env":       "production",
        "release":   "4.1.0",
        "exc_type":  "KeychainError",
        "exc_value": "SecItemAdd failed: errSecDuplicateItem (-25299)",
        "frames": frames_swift(
            ("Security",                 "SecItemAdd",                  "SecItem.m",              1, False, None),
            ("DeepReaderApp",            "storeToken",                  "AuthService.swift",      88, True, "        let status = SecItemAdd(query as CFDictionary, nil)"),
            ("DeepReaderApp",            "login(email:password:)",      "AuthService.swift",      55, True, "        try storeToken(token, account: email)"),
            ("DeepReaderApp",            "loginButtonTapped(_:)",       "LoginViewController.swift", 34, True, "        try await authService.login(email: email, password: password)"),
        ),
    },

    # 003 NSURLErrorDomain -1001 timeout
    {
        "issue_id":  "c0000003-0000-0000-0000-000000000003",
        "project_id":"b1b2c3d4-0002-0002-0002-000000000002",
        "ts":        "2026-06-09T10:00:00Z",
        "level":     "error",
        "env":       "production",
        "release":   "4.0.5",
        "exc_type":  "NSURLErrorDomain",
        "exc_value": "The request timed out. (code -1001)",
        "frames": frames_swift(
            ("CFNetwork",                "CFURLConnectionSendSynchronousRequest", "CFURLConnection.c", 1, False, None),
            ("Foundation",               "-[NSURLSession dataTaskWithRequest:]",  "NSURLSession.m",    1, False, None),
            ("DeepReaderApp",            "fetch(_:)",                            "NetworkClient.swift", 55, True, "        let (data, response) = try await URLSession.shared.data(for: request)"),
            ("DeepReaderApp",            "loadRecommendations()",                "HomeViewModel.swift", 88, True, "        recommendations = try await api.fetch(.recommendations)"),
        ),
    },

    # 005 Core Data merge conflict
    {
        "issue_id":  "c0000003-0000-0000-0000-000000000005",
        "project_id":"b1b2c3d4-0002-0002-0002-000000000002",
        "ts":        "2026-06-09T07:30:00Z",
        "level":     "error",
        "env":       "production",
        "release":   "4.1.1",
        "exc_type":  "NSMergeConflict",
        "exc_value": "Merge conflict on NSManagedObject at line 0: mergeByPropertyObjectTrump",
        "frames": frames_swift(
            ("CoreData",                 "NSManagedObjectContext.save",         "NSManagedObjectContext.m", 1, False, None),
            ("DeepReaderApp",            "save(context:)",                      "DataStack.swift",         190, True, "        try context.save()"),
            ("DeepReaderApp",            "persistHighlight(_:)",                "HighlightService.swift",   55, True, "        try dataStack.save(context: backgroundContext)"),
        ),
    },

    # 006 Memory warning level 2
    {
        "issue_id":  "c0000003-0000-0000-0000-000000000006",
        "project_id":"b1b2c3d4-0002-0002-0002-000000000002",
        "ts":        "2026-06-09T13:00:00Z",
        "level":     "fatal",
        "env":       "production",
        "release":   "4.1.1",
        "exc_type":  "EXC_CRASH",
        "exc_value": "Termination reason: memory pressure (jetsam) — level 2",
        "frames": frames_swift(
            ("UIKitCore",                "UIApplicationMain",                   "UIApplication.m",     1, False, None),
            ("DeepReaderApp",            "ImageCache.cacheImage(_:key:)",        "ImageCache.swift",    77, True, "        cache[key] = UIImage(data: data)  // unbounded cache"),
            ("DeepReaderApp",            "collectionView(_:cellForItemAt:)",     "FeedViewController.swift", 144, True, "        cell.imageView.image = imageCache.get(url: url)"),
        ),
    },

    # 009 SwiftUI @State mutation on background thread
    {
        "issue_id":  "c0000003-0000-0000-0000-000000000009",
        "project_id":"b1b2c3d4-0002-0002-0002-000000000002",
        "ts":        "2026-06-09T08:45:00Z",
        "level":     "error",
        "env":       "staging",
        "release":   "4.1.1",
        "exc_type":  "SwiftUI.MainThreadViolation",
        "exc_value": "Publishing changes from background threads is not allowed; make sure to publish values from the main thread",
        "frames": frames_swift(
            ("SwiftUI",                  "ViewGraph.updateOutputs(at:)",        "ViewGraph.swift",    1, False, None),
            ("DeepReaderApp",            "loadBooks()",                         "HomeViewModel.swift", 45, True, "        self.books = result  // mutates @Published on background Task"),
            ("DeepReaderApp",            "HomeView.body",                       "HomeView.swift",      88, True, "        .onAppear { viewModel.loadBooks() }"),
        ),
    },

    # 012 AVPlayer item failed to play
    {
        "issue_id":  "c0000003-0000-0000-0000-000000000012",
        "project_id":"b1b2c3d4-0002-0002-0002-000000000002",
        "ts":        "2026-06-09T11:00:00Z",
        "level":     "error",
        "env":       "production",
        "release":   "4.1.1",
        "exc_type":  "AVError",
        "exc_value": "AVPlayerItem.Status.failed — AVError.mediaServicesWereReset (code -11819)",
        "frames": frames_swift(
            ("AVFAudio",                 "AVPlayerItemObserving.observeValue",  "AVPlayerItem.m",       1, False, None),
            ("DeepReaderApp",            "playerItem(_:didFailWith:)",          "PlayerViewController.swift", 99, True, "        showError(player.currentItem?.error)"),
            ("DeepReaderApp",            "setupPlayer(url:)",                   "PlayerViewController.swift", 55, True, "        player.replaceCurrentItem(with: item)"),
        ),
    },

    # 013 Silent auth refresh loop
    {
        "issue_id":  "c0000003-0000-0000-0000-000000000013",
        "project_id":"b1b2c3d4-0002-0002-0002-000000000002",
        "ts":        "2026-06-09T14:00:00Z",
        "level":     "error",
        "env":       "production",
        "release":   "4.1.1",
        "exc_type":  "AuthError",
        "exc_value": "Token refresh failed: 401 Unauthorized — refresh token expired",
        "frames": frames_swift(
            ("Foundation",               "URLSession.dataTask(with:completionHandler:)", "NSURLSession.m", 1, False, None),
            ("DeepReaderApp",            "refreshToken()",                       "TokenRefresher.swift", 55, True, "        let (data, response) = try await session.data(for: refreshRequest)"),
            ("DeepReaderApp",            "intercept(request:)",                  "AuthInterceptor.swift", 33, True, "        try await tokenRefresher.refreshToken()  // called on every 401"),
        ),
    },

    # ── Android App ───────────────────────────────────────────────────────────

    # 001 ANR: main thread blocked
    {
        "issue_id":  "c0000004-0000-0000-0000-000000000001",
        "project_id":"b1b2c3d4-0003-0003-0003-000000000003",
        "ts":        "2026-06-09T12:00:00Z",
        "level":     "fatal",
        "env":       "production",
        "release":   "3.5.0",
        "exc_type":  "ANR",
        "exc_value": "ANR in com.example.app: Input dispatching timed out — main thread blocked for 5002ms",
        "frames": frames_kotlin(
            ("android.graphics",         "BitmapFactory.decodeFile",  "BitmapFactory.java",   600, False, None),
            ("com.example.app",          "loadThumbnail",             "ImageLoader.kt",        88, True,  "    val bmp = BitmapFactory.decodeFile(path)  // on main thread!"),
            ("com.example.app",          "onBindViewHolder",          "FeedAdapter.kt",        55, True,  "    holder.thumbnail.setImageBitmap(imageLoader.loadThumbnail(item.path))"),
        ),
    },

    # 002 ClassCastException in RecyclerView adapter
    {
        "issue_id":  "c0000004-0000-0000-0000-000000000002",
        "project_id":"b1b2c3d4-0003-0003-0003-000000000003",
        "ts":        "2026-06-09T10:00:00Z",
        "level":     "error",
        "env":       "production",
        "release":   "3.5.1",
        "exc_type":  "java.lang.ClassCastException",
        "exc_value": "HeaderViewHolder cannot be cast to ItemViewHolder",
        "frames": frames_kotlin(
            ("com.example.app",          "onBindViewHolder",          "FeedAdapter.kt",       144, True, "    val itemHolder = holder as ItemViewHolder  // wrong cast when position=0 (header)"),
            ("androidx.recyclerview",    "RecyclerView.dispatchLayout","RecyclerView.java",    4229, False, None),
        ),
    },

    # 003 WorkManager task dropped on Doze mode
    {
        "issue_id":  "c0000004-0000-0000-0000-000000000003",
        "project_id":"b1b2c3d4-0003-0003-0003-000000000003",
        "ts":        "2026-06-09T07:00:00Z",
        "level":     "warning",
        "env":       "production",
        "release":   "3.5.1",
        "exc_type":  "androidx.work.WorkerTimeoutException",
        "exc_value": "Worker SyncWorker exceeded 10 minute execution window and was stopped",
        "frames": frames_kotlin(
            ("androidx.work.impl",       "WorkerWrapper.runWorker",   "WorkerWrapper.kt",     233, False, None),
            ("com.example.app",          "doWork",                    "SyncWorker.kt",         67, True,  "    // No early exit on Doze — runs until killed"),
        ),
    },

    # 005 Firebase Crashlytics: uncaught exception on API<26
    {
        "issue_id":  "c0000004-0000-0000-0000-000000000005",
        "project_id":"b1b2c3d4-0003-0003-0003-000000000003",
        "ts":        "2026-06-09T09:30:00Z",
        "level":     "error",
        "env":       "production",
        "release":   "3.5.2",
        "exc_type":  "java.lang.NoSuchMethodError",
        "exc_value": "No virtual method createNotificationChannel on API 24",
        "frames": frames_kotlin(
            ("android.app",              "NotificationManager.createNotificationChannel", "NotificationManager.java", 1, False, None),
            ("com.example.app",          "createChannel",             "NotificationHelper.kt", 55, True, "    notificationManager.createNotificationChannel(channel)  // requires API 26+"),
            ("com.example.app",          "showSyncNotification",      "SyncWorker.kt",         88, True, "    NotificationHelper.createChannel(context)"),
        ),
    },

    # 006 Retrofit SSLHandshakeException on Android 7
    {
        "issue_id":  "c0000004-0000-0000-0000-000000000006",
        "project_id":"b1b2c3d4-0003-0003-0003-000000000003",
        "ts":        "2026-06-09T08:00:00Z",
        "level":     "error",
        "env":       "production",
        "release":   "3.5.2",
        "exc_type":  "javax.net.ssl.SSLHandshakeException",
        "exc_value": "Connection closed by peer — server likely requires TLS 1.3, Android 7 supports up to TLS 1.2",
        "frames": frames_kotlin(
            ("com.android.org.conscrypt", "NativeCrypto.SSL_do_handshake", "NativeCrypto.java", 1, False, None),
            ("okhttp3.internal.connection","RealConnection.connectTls",    "RealConnection.kt",  370, False, None),
            ("com.example.app",           "createClient",                 "ApiClient.kt",         29, True, "    return OkHttpClient.Builder().build()  // no TLS fallback configured"),
            ("com.example.app",           "getApiService",                "ApiClient.kt",         55, True, "    val client = createClient()"),
        ),
    },

    # 007 OOM in Glide image pipeline
    {
        "issue_id":  "c0000004-0000-0000-0000-000000000007",
        "project_id":"b1b2c3d4-0003-0003-0003-000000000003",
        "ts":        "2026-06-09T13:30:00Z",
        "level":     "fatal",
        "env":       "production",
        "release":   "3.5.2",
        "exc_type":  "java.lang.OutOfMemoryError",
        "exc_value": "Failed to allocate a 67108864 byte allocation with 25165824 free bytes",
        "frames": frames_kotlin(
            ("com.bumptech.glide",       "Engine.load",               "Engine.java",          282, False, None),
            ("com.bumptech.glide",       "RequestManager.into",       "RequestManager.java",  166, False, None),
            ("com.example.app",          "bindCoverImage",            "FeedAdapter.kt",        88, True, "    Glide.with(context).load(url).into(holder.coverImageView)"),
        ),
    },

    # 009 ViewModel leaked
    {
        "issue_id":  "c0000004-0000-0000-0000-000000000009",
        "project_id":"b1b2c3d4-0003-0003-0003-000000000003",
        "ts":        "2026-06-09T11:00:00Z",
        "level":     "error",
        "env":       "production",
        "release":   "3.5.1",
        "exc_type":  "kotlinx.coroutines.JobCancellationException",
        "exc_value": "Parent job is Completed — ViewModel scope closed while search still in flight",
        "frames": frames_kotlin(
            ("kotlinx.coroutines",       "JobSupport.cancel",         "JobSupport.kt",         793, False, None),
            ("com.example.app",          "search",                    "SearchViewModel.kt",     77, True, "    viewModelScope.launch { results.value = repo.search(query) }  // scope may be cancelled"),
            ("com.example.app",          "onQueryTextChange",         "SearchFragment.kt",      44, True, "    viewModel.search(newText)"),
        ),
    },

    # 012 Crash on back press: fragment backstack corrupt
    {
        "issue_id":  "c0000004-0000-0000-0000-000000000012",
        "project_id":"b1b2c3d4-0003-0003-0003-000000000003",
        "ts":        "2026-06-09T10:45:00Z",
        "level":     "error",
        "env":       "production",
        "release":   "3.5.2",
        "exc_type":  "java.lang.IllegalArgumentException",
        "exc_value": "No destination with ID 0x7f0b0055 is on the back stack",
        "frames": frames_kotlin(
            ("androidx.navigation",      "NavController.popBackStack",     "NavController.kt",  577, False, None),
            ("com.example.app",          "onBackPressed",                  "NavController.kt",   99, True,  "    navController.popBackStack(R.id.homeFragment, false)"),
        ),
    },

    # 013 Hilt injection failed
    {
        "issue_id":  "c0000004-0000-0000-0000-000000000013",
        "project_id":"b1b2c3d4-0003-0003-0003-000000000003",
        "ts":        "2026-06-09T14:30:00Z",
        "level":     "error",
        "env":       "staging",
        "release":   "3.6.0-beta",
        "exc_type":  "dagger.hilt.internal.GeneratedComponentManagerHolder",
        "exc_value": "MissingBinding: com.example.app.logging.Logger cannot be provided without an @Inject constructor",
        "frames": frames_kotlin(
            ("dagger.hilt.internal",     "Hilt_AppComponent.initialize","Hilt_AppComponent.java", 1, False, None),
            ("com.example.app",          "onCreate",                    "AppComponent.kt",         11, True, "    // Logger binding missing after refactor"),
            ("android.app",              "Application.onCreate",        "Application.java",        148, False, None),
        ),
    },

    # 014 ExoPlayer MediaCodec error
    {
        "issue_id":  "c0000004-0000-0000-0000-000000000014",
        "project_id":"b1b2c3d4-0003-0003-0003-000000000003",
        "ts":        "2026-06-09T07:00:00Z",
        "level":     "error",
        "env":       "production",
        "release":   "3.5.1",
        "exc_type":  "com.google.android.exoplayer2.ExoPlaybackException",
        "exc_value": "MediaCodec.dequeueOutputBuffer returned -1 (ERROR_TIMEOUT) for decoder OMX.qcom.video.decoder.avc",
        "frames": frames_kotlin(
            ("com.google.android.exoplayer2", "MediaCodecRenderer.drainOutputBuffer", "MediaCodecRenderer.java", 1652, False, None),
            ("com.example.app",              "onPlayerError",                          "VideoPlayer.kt",          166, True, "    Timber.e(error, \"ExoPlayer error\")\n    showErrorDialog()"),
        ),
    },
]


# ---------------------------------------------------------------------------
# Insert
# ---------------------------------------------------------------------------

def build_payload(ev: dict) -> dict:
    return {
        "event_id": ev["event_id_hex"],
        "timestamp": ev["ts"],
        "level":     ev["level"],
        "environment": ev["env"],
        "release":   ev["release"],
        "exception": {
            "values": [{
                "type":       ev["exc_type"],
                "value":      ev["exc_value"],
                "stacktrace": {"frames": ev["frames"]},
            }]
        },
    }


def main() -> None:
    if not os.path.exists(DB_FILE):
        print(f"error: database file '{DB_FILE}' not found", file=sys.stderr)
        sys.exit(1)

    con = sqlite3.connect(DB_FILE)
    cur = con.cursor()

    inserted = 0
    skipped  = 0

    for idx, ev in enumerate(EVENTS):
        ev["event_id_hex"] = hex32(idx + 1, group=idx // 100 + 1)
        internal_id = uid(idx + 1, group=idx // 100 + 1)
        payload_str = json.dumps(build_payload(ev), ensure_ascii=False)

        try:
            cur.execute(
                """
                INSERT OR IGNORE INTO events (id, event_id, issue_id, project_id, payload, received_at)
                VALUES (?, ?, ?, ?, ?, ?)
                """,
                (internal_id, ev["event_id_hex"], ev["issue_id"], ev["project_id"], payload_str, ev["ts"]),
            )
            if cur.rowcount:
                inserted += 1
            else:
                skipped += 1
        except sqlite3.Error as e:
            print(f"error inserting event {internal_id}: {e}", file=sys.stderr)

    con.commit()
    con.close()

    total = inserted + skipped
    print(f"events:   {total} total ({inserted} inserted, {skipped} already existed)")


if __name__ == "__main__":
    main()
