# Printing implementation notes

Printing is not implemented. The toolbar control remains disabled.

The installed Wry WebView2 print method executes `window.print()`. In this app that prints the React workspace and its visible virtualized pages, so it cannot serve as document printing.

The next implementation should use a native Windows print dialog and PDFium raster output, fitting each page to the printable area. Raster output reuses the public renderer but will not preserve vector fidelity. A separate native spool thread must own the printer handles and dialog; printer/network calls must not block the UI or PDF worker.

1. Use [PrintDlgExW](https://learn.microsoft.com/en-us/windows/win32/api/commdlg/ns-commdlg-printdlgexw) for printer, range, paper, copies and cancellation. Start output only for its Print result, never Apply or Cancel.
2. Snapshot the current page plan and validate its revision. Keep the underlying document alive for the job. The snapshot must preserve rotation, order and deletion and must not change the document's saved/dirty state.
3. Render one page at a time through the existing PDF worker, using print quality, annotations, a white background, explicit BGRA and a bounded pixel allocation. Pass bytes directly to the spool thread, not through React.
4. Use [GDI printing](https://learn.microsoft.com/en-us/windows/win32/printdocs/gdi-print-api-functions) with deterministic handle cleanup. Cancellation must reach the spooler without waiting behind rendering, aborting failed or canceled jobs. Already printed paper cannot be recalled.
5. Check encrypted-document print permissions explicitly. Reject unknown permission states until support is verified.

Before enabling the control, test snapshot stability, edited-page order/rotation, all/current/disjoint ranges, mixed page sizes, bitmap orientation/colors/stride, allocation limits, cancellation, printer errors and unchanged dirty state through a fake spool sink. Then inspect Microsoft Print to PDF output and exercise at least one real printer. Desktop automation is currently unreliable, so those native acceptance checks remain blocked.
