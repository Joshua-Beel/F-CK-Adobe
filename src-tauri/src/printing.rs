use crate::service::PrintBitmap;
use serde::Serialize;
use std::sync::{atomic::{AtomicBool, Ordering}, Arc};

#[derive(Debug, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PrintOutcome { Cancelled, Submitted { pages: usize } }

fn selected_pages(count: usize, current: usize, mode: u8, ranges: &[(u32, u32)]) -> Result<Vec<usize>, String> {
    if count == 0 || count > 65536 || current >= count { return Err("Invalid print page count or current page.".into()); }
    match mode {
        0 => Ok((0..count).collect()),
        1 => Ok(vec![current]),
        2 => {
            if ranges.is_empty() { return Err("Choose at least one page to print.".into()); }
            let mut selected = vec![false; count];
            for &(first, last) in ranges {
                if first == 0 || last < first || last as usize > count { return Err("Print range is outside the document.".into()); }
                selected[first as usize - 1..last as usize].fill(true);
            }
            Ok(selected.into_iter().enumerate().filter_map(|(i, yes)| yes.then_some(i)).collect())
        }
        _ => Err("Unsupported print selection.".into()),
    }
}

fn validate_bitmap(bitmap: &PrintBitmap) -> Result<(), String> {
    let pixels = u64::from(bitmap.width) * u64::from(bitmap.height);
    if bitmap.width == 0 || bitmap.height == 0 || bitmap.width > 4096 || bitmap.height > 4096
        || pixels > 16_000_000 || pixels * 4 != bitmap.bgra.len() as u64 {
        return Err("Invalid or oversized print bitmap.".into());
    }
    Ok(())
}

fn raster_bounds(width: i32, height: i32, dpi_x: i32, dpi_y: i32) -> Result<(u32, u32), String> {
    if width <= 0 || height <= 0 || dpi_x <= 0 || dpi_y <= 0 { return Err("Printer reported invalid paper dimensions.".into()); }
    let w = width as f64 * 300.0 / dpi_x as f64;
    let h = height as f64 * 300.0 / dpi_y as f64;
    let scale = 1.0_f64.min(4096.0 / w).min(4096.0 / h).min((16_000_000.0 / (w * h)).sqrt());
    Ok(((w * scale).floor().max(1.0) as u32, (h * scale).floor().max(1.0) as u32))
}

fn fit(width: i32, height: i32, dpi_x: i32, dpi_y: i32, bitmap: &PrintBitmap) -> (i32, i32, i32, i32) {
    let w = bitmap.width as f64 * dpi_x as f64;
    let h = bitmap.height as f64 * dpi_y as f64;
    let scale = (width as f64 / w).min(height as f64 / h);
    let fitted_w = (w * scale).floor().max(1.0) as i32;
    let fitted_h = (h * scale).floor().max(1.0) as i32;
    ((width - fitted_w) / 2, (height - fitted_h) / 2, fitted_w, fitted_h)
}

pub fn print(hwnd: isize, title: &str, page_count: usize, current_page: usize, cancel: Arc<AtomicBool>, render: impl FnMut(usize, u32, u32) -> Result<PrintBitmap, String>) -> Result<PrintOutcome, String> {
    #[cfg(windows)]
    { native::print(hwnd, title, page_count, current_page, cancel, render) }
    #[cfg(not(windows))]
    { let _ = (hwnd, title, page_count, current_page, cancel, render); Err("Native printing is available on Windows only.".into()) }
}

#[cfg(windows)]
mod native {
    use super::*;
    use std::{cell::RefCell, mem::size_of};
    use windows::{core::{BOOL, PCWSTR}, Win32::{Foundation::{GlobalFree, HWND}, Graphics::Gdi::*, Storage::Xps::*, System::Com::*, UI::Controls::Dialogs::*}};

    thread_local! { static CANCEL: RefCell<Option<Arc<AtomicBool>>> = const { RefCell::new(None) }; }
    unsafe extern "system" fn abort_proc(_: HDC, _: i32) -> BOOL {
        BOOL::from(CANCEL.with(|slot| slot.borrow().as_ref().is_some_and(|flag| !flag.load(Ordering::Relaxed))))
    }
    struct Com;
    impl Drop for Com { fn drop(&mut self) { unsafe { CoUninitialize(); } } }
    struct Cancellation;
    impl Drop for Cancellation { fn drop(&mut self) { CANCEL.with(|slot| *slot.borrow_mut() = None); } }
    struct Dialog(PRINTDLGEXW);
    impl Drop for Dialog {
        fn drop(&mut self) { unsafe {
            if !self.0.hDC.0.is_null() { let _ = DeleteDC(self.0.hDC); }
            if !self.0.hDevMode.0.is_null() { let _ = GlobalFree(Some(self.0.hDevMode)); }
            if !self.0.hDevNames.0.is_null() { let _ = GlobalFree(Some(self.0.hDevNames)); }
        } }
    }
    struct Job { dc: HDC, active: bool }
    impl Drop for Job { fn drop(&mut self) { if self.active { unsafe { AbortDoc(self.dc); } } } }
    fn check(value: i32, operation: &str) -> Result<(), String> {
        if value <= 0 { Err(format!("Printer failed during {operation}.")) } else { Ok(()) }
    }

    pub(super) fn print(hwnd: isize, title: &str, count: usize, current: usize, cancel: Arc<AtomicBool>, mut render: impl FnMut(usize, u32, u32) -> Result<PrintBitmap, String>) -> Result<PrintOutcome, String> {
        if count == 0 || count > 65536 || current >= count || hwnd == 0 { return Err("Invalid print document or owner window.".into()); }
        if cancel.load(Ordering::Relaxed) { return Ok(PrintOutcome::Cancelled); }
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().map_err(|e| format!("Could not initialize print dialog: {e}"))?; }
        let _com = Com;
        let mut ranges = vec![PRINTPAGERANGE::default(); 100];
        let mut dialog = Dialog(PRINTDLGEXW {
            lStructSize: size_of::<PRINTDLGEXW>() as u32,
            hwndOwner: HWND(hwnd as *mut _),
            Flags: PD_RETURNDC | PD_NOSELECTION | PD_USEDEVMODECOPIESANDCOLLATE | PD_HIDEPRINTTOFILE,
            nMinPage: 1, nMaxPage: count as u32, nCopies: 1,
            nMaxPageRanges: ranges.len() as u32, lpPageRanges: ranges.as_mut_ptr(),
            nStartPage: START_PAGE_GENERAL,
            ..Default::default()
        });
        unsafe { PrintDlgExW(&mut dialog.0).map_err(|e| format!("Could not open print dialog: {e}"))?; }
        if dialog.0.dwResultAction != PD_RESULT_PRINT || cancel.load(Ordering::Relaxed) { return Ok(PrintOutcome::Cancelled); }
        if dialog.0.hDC.0.is_null() { return Err("Printer did not provide a drawing context.".into()); }
        let mode = if dialog.0.Flags.contains(PD_CURRENTPAGE) { 1 } else if dialog.0.Flags.contains(PD_PAGENUMS) { 2 } else { 0 };
        let used = ranges.get(..dialog.0.nPageRanges as usize).ok_or("Printer returned too many page ranges.")?;
        let selected = selected_pages(count, current, mode, &used.iter().map(|r| (r.nFromPage, r.nToPage)).collect::<Vec<_>>())?;
        let dc = dialog.0.hDC;
        let (width, height, dpi_x, dpi_y) = unsafe { (GetDeviceCaps(Some(dc), HORZRES), GetDeviceCaps(Some(dc), VERTRES), GetDeviceCaps(Some(dc), LOGPIXELSX), GetDeviceCaps(Some(dc), LOGPIXELSY)) };
        let (max_w, max_h) = raster_bounds(width, height, dpi_x, dpi_y)?;
        let already_printing = CANCEL.with(|slot| slot.borrow().is_some());
        if already_printing { return Err("A print job is already running on this thread.".into()); }
        CANCEL.with(|slot| *slot.borrow_mut() = Some(cancel.clone()));
        let _cancellation = Cancellation;
        let title: Vec<u16> = title.chars().filter(|c| *c != '\0').take(240).collect::<String>().encode_utf16().chain(Some(0)).collect();
        let info = DOCINFOW { cbSize: size_of::<DOCINFOW>() as i32, lpszDocName: PCWSTR(title.as_ptr()), ..Default::default() };
        let mut job = Job { dc, active: false };
        let result = (|| {
            unsafe {
                check(SetAbortProc(dc, Some(abort_proc)), "cancellation setup")?;
                check(StartDocW(dc, &info), "starting the job")?;
            }
            job.active = true;
            for &page in &selected {
                if cancel.load(Ordering::Relaxed) { return Ok(PrintOutcome::Cancelled); }
                let bitmap = render(page, max_w, max_h)?;
                validate_bitmap(&bitmap)?;
                if cancel.load(Ordering::Relaxed) { return Ok(PrintOutcome::Cancelled); }
                let (x, y, w, h) = fit(width, height, dpi_x, dpi_y, &bitmap);
                let bitmap_info = BITMAPINFO { bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32, biWidth: bitmap.width as i32,
                    biHeight: -(bitmap.height as i32), biPlanes: 1, biBitCount: 32,
                    biCompression: BI_RGB.0, ..Default::default()
                }, ..Default::default() };
                unsafe {
                    check(StartPage(dc), "starting a page")?;
                    check(StretchDIBits(dc, x, y, w, h, 0, 0, bitmap.width as i32, bitmap.height as i32, Some(bitmap.bgra.as_ptr().cast()), &bitmap_info, DIB_RGB_COLORS, SRCCOPY), "drawing a page")?;
                    check(EndPage(dc), "finishing a page")?;
                }
            }
            if cancel.load(Ordering::Relaxed) { return Ok(PrintOutcome::Cancelled); }
            unsafe { check(EndDoc(dc), "finishing the job")?; }
            job.active = false;
            Ok(PrintOutcome::Submitted { pages: selected.len() })
        })();
        if result.is_err() && cancel.load(Ordering::Relaxed) { Ok(PrintOutcome::Cancelled) } else { result }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn page_ranges_are_bounded_and_deduplicated() {
        assert_eq!(selected_pages(5, 3, 0, &[]).unwrap(), vec![0,1,2,3,4]);
        assert_eq!(selected_pages(5, 3, 1, &[]).unwrap(), vec![3]);
        assert_eq!(selected_pages(5, 0, 2, &[(4,5),(1,2),(2,2)]).unwrap(), vec![0,1,3,4]);
        for ranges in [vec![], vec![(0,1)], vec![(3,2)], vec![(1,6)]] { assert!(selected_pages(5,0,2,&ranges).is_err()); }
        assert!(selected_pages(0,0,0,&[]).is_err());
        assert!(selected_pages(3,3,1,&[]).is_err());
        assert!(selected_pages(usize::MAX,0,0,&[]).is_err());
        assert_eq!(selected_pages(65536,65535,1,&[]).unwrap(), vec![65535]);
        assert_eq!(selected_pages(65536,0,2,&[(65536,65536)]).unwrap(), vec![65535]);
        assert!(selected_pages(65537,0,0,&[]).is_err());
    }
    #[test]
    fn raster_limits_and_asymmetric_printer_resolution() {
        assert_eq!(raster_bounds(2400,3300,300,300).unwrap(), (2400,3300));
        let (w,h) = raster_bounds(i32::MAX,i32::MAX,1,1).unwrap();
        assert!(w <= 4096 && h <= 4096 && u64::from(w)*u64::from(h) <= 16_000_000);
        assert!(raster_bounds(1,1,0,300).is_err());
        let bitmap = PrintBitmap { width: 100, height: 100, bgra: vec![255;40000] };
        assert_eq!(fit(1200,1200,600,300,&bitmap), (0,300,1200,600));
        assert!(validate_bitmap(&bitmap).is_ok());
        assert!(validate_bitmap(&PrintBitmap { width: 100,height: 100,bgra: vec![0;4] }).is_err());
        assert!(validate_bitmap(&PrintBitmap { width: u32::MAX,height: u32::MAX,bgra: vec![] }).is_err());
    }
}
