# Printing implementation notes

Native Windows printing is implemented in the source build through the toolbar and Ctrl+P. It is not included in the 0.2.4 draft. Actual Windows dialog interaction, physical output and printer-driver cancellation remain unverified.

The installed Wry WebView2 print method executes `window.print()`. In this app that prints the React workspace and its visible virtualized pages, so it cannot serve as document printing.

The implementation uses a native Windows print dialog and PDFium raster output, fitting each page to the printable area. Raster output reuses the public renderer but does not preserve vector fidelity. A dedicated native spool thread owns printer handles and the dialog; printer/network calls do not run on the UI or PDF worker.

1. [PrintDlgExW](https://learn.microsoft.com/en-us/windows/win32/api/commdlg/ns-commdlg-printdlgexw) supplies printer, range, paper, copies and cancellation. Output starts only for its Print result, never Apply or Cancel.
2. A snapshot preserves the current page plan after validating its revision and pins the underlying document until cleanup. Its non-cloneable lease queues idempotent cleanup if an unread reply is dropped; the spool thread owns that lease through completion or panic, while normal completion awaits cleanup before reporting. Later edits or closure cannot change the job. Printing does not mark the document saved.
3. The PDF worker renders one page at a time with print quality, annotations/forms, white background and packed BGRA. Output is limited to 4,096 pixels on either axis and 16 million pixels per page, targeting 300 dpi when that fits. Bytes go directly to the spool thread.
4. [GDI printing](https://learn.microsoft.com/en-us/windows/win32/printdocs/gdi-print-api-functions) handles spooling with abort and handle cleanup. The cancellation flag is independent of the PDF queue. Submitted pages may still print after cancellation. The UI reports submission, not physical completion.
5. All encrypted or unknown-security files are rejected for printing in this build.

Automated coverage includes snapshot stability through edits/closure, unread-reply capacity release, spool-thread handoff, normal completion and panic cleanup, order/rotation, all/current/disjoint page selection, BGRA color channels and white background, allocation limits, asymmetric printer DPI, early cancellation, resource-reservation lifetime, UI retry and late cancellation replies. GDI spooling itself is not mocked end to end. Pending cancellation IDs are bounded to 64 and expire after five minutes if no job reserves them.

The snapshot lease covers the worker-to-service reply handoff. It does not establish that a later Tauri, WebView, or JavaScript recipient received the completed print result.

Still required: inspect Microsoft Print to PDF output, exercise a physical printer, verify mixed-size/orientation output, and induce actual printer/driver errors and cancellation. Desktop automation is currently unreliable, so these native acceptance checks remain blocked.
