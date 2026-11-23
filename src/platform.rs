use crate::{Error, Result};
use std::path::Path;

#[cfg(windows)]
pub(crate) fn wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

#[cfg(windows)]
mod shell {
    use super::*;
    use windows::{
        Win32::{
            Storage::FileSystem::{GetDriveTypeW, GetVolumePathNameW},
            System::Com::{
                CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
                CoTaskMemFree, CoUninitialize, IBindCtx,
            },
            UI::Shell::{
                Common::ITEMIDLIST, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT,
                FOFX_EARLYFAILURE, FOFX_RECYCLEONDELETE, FileOperation, IFileOperation,
                IFileOperationProgressSink, IShellItem, SHCreateItemFromParsingName,
                SHOpenFolderAndSelectItems, SHParseDisplayName, SHQUERYRBINFO, SHQueryRecycleBinW,
                ShellExecuteW,
            },
            UI::WindowsAndMessaging::SW_SHOWNORMAL,
        },
        core::{PCWSTR, w},
    };

    struct Apartment;

    impl Apartment {
        fn open() -> windows::core::Result<Self> {
            unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()? };
            Ok(Self)
        }
    }

    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }

    pub fn open_folder(path: &Path) -> Result<()> {
        if !crate::disk::required(path)?.is_dir() {
            return Err(Error::message_at(path, "请选择目录"));
        }
        let name = wide(path);
        let result = unsafe {
            ShellExecuteW(
                None,
                w!("open"),
                PCWSTR(name.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        if result.0 as isize <= 32 {
            return Err(Error::message_at(
                path,
                format!("资源管理器错误 {}", result.0 as isize),
            ));
        }
        Ok(())
    }

    pub fn select_file(path: &Path) -> Result<()> {
        if !crate::disk::required(path)?.is_file() {
            return Err(Error::message_at(path, "请选择文件"));
        }
        show_in_explorer(path)
    }

    fn show_in_explorer(path: &Path) -> Result<()> {
        let result = (|| unsafe {
            let _apartment = Apartment::open()?;
            let name = wide(path);
            let mut item: *mut ITEMIDLIST = std::ptr::null_mut();
            SHParseDisplayName(PCWSTR(name.as_ptr()), None::<&IBindCtx>, &mut item, 0, None)?;
            let selected = SHOpenFolderAndSelectItems(item, None, 0);
            CoTaskMemFree(Some(item.cast()));
            selected
        })();
        result.map_err(|error| Error::message_at(path, error))
    }

    pub fn recycle(path: &Path) -> Result<()> {
        let name = wide(path);
        let mut volume = vec![0u16; 32768];
        unsafe { GetVolumePathNameW(PCWSTR(name.as_ptr()), &mut volume) }
            .map_err(|error| Error::message_at(path, error))?;
        // DRIVE_FIXED. Avoid the shell's permanent-delete paths for remote
        // and removable volumes.
        if unsafe { GetDriveTypeW(PCWSTR(volume.as_ptr())) } != 3 {
            return Err(Error::message_at(path, "此位置不支持回收站"));
        }
        let mut info = SHQUERYRBINFO {
            cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32,
            ..Default::default()
        };
        unsafe { SHQueryRecycleBinW(PCWSTR(volume.as_ptr()), &mut info) }
            .map_err(|error| Error::message_at(path, error))?;
        let result: windows::core::Result<bool> = (|| unsafe {
            let _apartment = Apartment::open()?;
            let operation: IFileOperation =
                CoCreateInstance(&FileOperation, None, CLSCTX_INPROC_SERVER)?;
            let item: IShellItem =
                SHCreateItemFromParsingName(PCWSTR(name.as_ptr()), None::<&IBindCtx>)?;
            operation.SetOperationFlags(
                FOFX_RECYCLEONDELETE
                    | FOFX_EARLYFAILURE
                    | FOF_NOERRORUI
                    | FOF_NOCONFIRMATION
                    | FOF_SILENT,
            )?;
            operation.DeleteItem(&item, None::<&IFileOperationProgressSink>)?;
            operation.PerformOperations()?;
            Ok(operation.GetAnyOperationsAborted()?.as_bool())
        })();
        if result.map_err(|error| Error::message_at(path, error))?
            || crate::disk::metadata(path)?.is_some()
        {
            return Err(Error::message_at(path, "Windows 未将快照移入回收站"));
        }
        Ok(())
    }
}

#[cfg(windows)]
pub use shell::{open_folder, recycle, select_file};

#[cfg(not(windows))]
pub fn open_folder(path: &Path) -> Result<()> {
    Err(Error::message_at(path, "仅支持 Windows 资源管理器"))
}

#[cfg(not(windows))]
pub fn select_file(path: &Path) -> Result<()> {
    open_folder(path)
}

#[cfg(not(windows))]
pub fn recycle(path: &Path) -> Result<()> {
    Err(Error::message_at(path, "仅支持 Windows 回收站"))
}
