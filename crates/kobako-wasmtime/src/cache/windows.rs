//! The cache directory on Windows, held open without sharing deletion, so it
//! is neither renamed nor replaced while held. An entry is reached by its path
//! beneath it, so each one opened is checked to stand in the held directory
//! itself; Windows lets anyone bypass traversal, so an entry is trusted only
//! as its directory is.

use std::ffi::{c_void, OsStr, OsString};
use std::fs;
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use windows_sys::Win32::Foundation::{
    CloseHandle, LocalFree, ERROR_ALREADY_EXISTS, GENERIC_ALL, GENERIC_WRITE, HANDLE,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo,
    SDDL_REVISION_1, SE_FILE_OBJECT,
};
use windows_sys::Win32::Security::{
    EqualSid, GetAce, GetTokenInformation, IsWellKnownSid, TokenUser, WinBuiltinAdministratorsSid,
    WinLocalSystemSid, ACCESS_ALLOWED_ACE, ACL, DACL_SECURITY_INFORMATION,
    OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES, TOKEN_QUERY,
    TOKEN_USER,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateDirectoryW, GetFinalPathNameByHandleW, DELETE, FILE_APPEND_DATA,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_DELETE_CHILD, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_EXECUTE, FILE_GENERIC_READ, FILE_LIST_DIRECTORY,
    FILE_NAME_NORMALIZED, FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
    FILE_WRITE_ATTRIBUTES, FILE_WRITE_DATA, FILE_WRITE_EA, READ_CONTROL, WRITE_DAC, WRITE_OWNER,
};
use windows_sys::Win32::System::SystemServices::ACCESS_ALLOWED_ACE_TYPE;
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// Every right that changes what an object holds or who may change it.
const WRITE_RIGHTS: u32 = FILE_WRITE_DATA
    | FILE_APPEND_DATA
    | FILE_WRITE_EA
    | FILE_DELETE_CHILD
    | FILE_WRITE_ATTRIBUTES
    | DELETE
    | WRITE_DAC
    | WRITE_OWNER
    | GENERIC_WRITE
    | GENERIC_ALL;

pub(super) struct PrivateDir {
    path: PathBuf,
    /// Held so the directory stays where it was checked.
    _held: fs::File,
    /// Where the directory stands, as each entry opened is compared to it.
    resolved: PathBuf,
}

impl PrivateDir {
    /// Create `dir` and the directories above it; `dir` itself takes a
    /// protected DACL granting only the current user, SYSTEM, and the
    /// Administrators, so no other local user can plant an artifact the
    /// unsafe deserialize would trust.
    pub(super) fn create(dir: &Path) -> io::Result<()> {
        if let Some(parent) = dir.parent() {
            fs::create_dir_all(parent)?;
        }
        let user = current_user()?;
        let sddl = format!(
            "D:P(A;OICI;FA;;;{})(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)",
            user.string()?
        );
        let descriptor = Descriptor::from_sddl(&sddl)?;
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: 0,
        };
        let wide = wide(dir.as_os_str());
        // SAFETY: `wide` is NUL-terminated and `attributes` points at a
        // descriptor that lives until the call returns.
        if unsafe { CreateDirectoryW(wide.as_ptr(), &attributes) } != 0 {
            return Ok(());
        }
        match io::Error::last_os_error() {
            err if err.raw_os_error() == Some(ERROR_ALREADY_EXISTS as i32) => Ok(()),
            err => Err(err),
        }
    }

    /// `dir`, once it upholds the trust the unsafe deserialize relies on: a
    /// directory rather than a link in its place, owned by the current user,
    /// and writable by no one else but SYSTEM and the Administrators, who may
    /// write anything.
    pub(super) fn open(dir: &Path) -> Option<Self> {
        let held = fs::OpenOptions::new()
            .access_mode(FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | READ_CONTROL)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(dir)
            .ok()?;
        let metadata = held.metadata().ok()?;
        if !metadata.is_dir() || is_reparse_point(&metadata) || !trusted(&held) {
            return None;
        }
        let resolved = resolved_path(&held)?;
        Some(Self {
            path: dir.to_path_buf(),
            _held: held,
            resolved,
        })
    }

    /// The entry `name`, opened without following a link as wasmtime opens a
    /// file to map: readable and executable, its code mapped as such, and
    /// shared only with readers, so its bytes stay as read while mapped. It
    /// may also refresh its own time.
    pub(super) fn open_entry(&self, name: &OsStr) -> Option<fs::File> {
        let file = fs::OpenOptions::new()
            .access_mode(FILE_GENERIC_READ | FILE_GENERIC_EXECUTE | FILE_WRITE_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(self.path.join(name))
            .ok()?;
        let metadata = file.metadata().ok()?;
        let beneath = resolved_path(&file)?.parent() == Some(self.resolved.as_path());
        (metadata.is_file() && !is_reparse_point(&metadata) && beneath && trusted(&file))
            .then_some(file)
    }

    /// A temp file no other writer shares: two threads of one process storing
    /// the same artifact at once would otherwise write into one file, and a
    /// reader could load the interleaved bytes the unsafe deserialize trusts.
    pub(super) fn create_temp(&self, name: &OsStr) -> Option<(PathBuf, fs::File)> {
        static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
        let serial = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let tmp = Path::new(name).with_extension(format!("tmp{}.{serial}", std::process::id()));
        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(self.path.join(&tmp))
            .ok()?;
        Some((tmp, file))
    }

    /// Move `tmp` over `name`; whether it moved. One another process holds
    /// open stays, as Windows refuses to replace it.
    pub(super) fn rename(&self, tmp: &Path, name: &OsStr) -> bool {
        fs::rename(self.path.join(tmp), self.path.join(name)).is_ok()
    }

    /// Each entry's name and when it was last modified, read without
    /// following a link; one whose time cannot be read is left out.
    pub(super) fn entries(&self) -> Vec<(OsString, SystemTime)> {
        let Ok(entries) = fs::read_dir(&self.path) else {
            return Vec::new();
        };
        entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let modified = fs::symlink_metadata(entry.path()).ok()?.modified().ok()?;
                Some((entry.file_name(), modified))
            })
            .collect()
    }

    pub(super) fn remove(&self, name: &OsStr) {
        let _ = fs::remove_file(self.path.join(name));
    }
}

fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

/// Where `file` stands once every link on its way is resolved.
fn resolved_path(file: &fs::File) -> Option<PathBuf> {
    let handle = file.as_raw_handle() as HANDLE;
    let mut buf = vec![0u16; 512];
    loop {
        // SAFETY: `buf` holds `buf.len()` wide characters for the call to fill.
        let len = unsafe {
            GetFinalPathNameByHandleW(
                handle,
                buf.as_mut_ptr(),
                buf.len() as u32,
                FILE_NAME_NORMALIZED,
            )
        } as usize;
        match len {
            0 => return None,
            len if len < buf.len() => return Some(OsString::from_wide(&buf[..len]).into()),
            len => buf.resize(len, 0),
        }
    }
}

/// Whether only the user, SYSTEM, and the Administrators own `file` or hold
/// a right to change it, its inheritable entries included, as what they
/// grant reaches each entry created beneath. An owner may grant itself any
/// right, and an elevated process's objects are owned by the Administrators.
fn trusted(file: &fs::File) -> bool {
    let Ok(user) = current_user() else {
        return false;
    };
    let mut owner: PSID = ptr::null_mut();
    let mut dacl: *mut ACL = ptr::null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SAFETY: each out-pointer is valid; `descriptor` owns the owner and the
    // DACL, and is freed below.
    let status = unsafe {
        GetSecurityInfo(
            file.as_raw_handle() as HANDLE,
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            ptr::null_mut(),
            &mut dacl,
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    if status != 0 {
        return false;
    }
    let descriptor = Descriptor(descriptor);
    // SAFETY: `owner` and `dacl` point into `descriptor`, alive for the scope.
    let trusted =
        unsafe { user.permits(owner) && !dacl.is_null() && grants_write_only_to(dacl, &user) };
    drop(descriptor);
    trusted
}

/// Whether every allowing entry of `dacl` that grants a right to change the
/// object names the user, SYSTEM, or the Administrators.
///
/// # Safety
///
/// `dacl` must point at a valid ACL.
unsafe fn grants_write_only_to(dacl: *mut ACL, user: &User) -> bool {
    for index in 0..unsafe { (*dacl).AceCount } {
        let mut ace: *mut c_void = ptr::null_mut();
        // SAFETY: `index` is below the ACL's count.
        if unsafe { GetAce(dacl, u32::from(index), &mut ace) } == 0 {
            return false;
        }
        let allowed = ace.cast::<ACCESS_ALLOWED_ACE>();
        // SAFETY: every ACE starts with its header; an allowing one carries
        // its mask and SID after it.
        let (kind, mask) = unsafe { ((*allowed).Header.AceType, (*allowed).Mask) };
        if u32::from(kind) != ACCESS_ALLOWED_ACE_TYPE || mask & WRITE_RIGHTS == 0 {
            continue;
        }
        let sid: PSID = unsafe { ptr::addr_of_mut!((*allowed).SidStart).cast() };
        // SAFETY: `sid` is the SID the allowing entry holds.
        if !unsafe { user.permits(sid) } {
            return false;
        }
    }
    true
}

/// The current process's user, as its token names it.
struct User(Vec<u8>);

impl User {
    fn sid(&self) -> PSID {
        // SAFETY: the buffer holds a `TOKEN_USER` whose SID points within it.
        unsafe { (*self.0.as_ptr().cast::<TOKEN_USER>()).User.Sid }
    }

    /// Whether `sid` is the user, SYSTEM, or the Administrators.
    ///
    /// # Safety
    ///
    /// `sid` must point at a valid SID.
    unsafe fn permits(&self, sid: PSID) -> bool {
        unsafe {
            EqualSid(sid, self.sid()) != 0
                || IsWellKnownSid(sid, WinLocalSystemSid) != 0
                || IsWellKnownSid(sid, WinBuiltinAdministratorsSid) != 0
        }
    }

    /// The SID as SDDL writes it.
    fn string(&self) -> io::Result<String> {
        let mut text = ptr::null_mut();
        // SAFETY: `text` receives a buffer freed below.
        if unsafe { ConvertSidToStringSidW(self.sid(), &mut text) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `text` is NUL-terminated.
        let len = (0..).take_while(|&i| unsafe { *text.add(i) } != 0).count();
        let string = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, len) });
        unsafe { LocalFree(text.cast()) };
        Ok(string)
    }
}

fn current_user() -> io::Result<User> {
    let mut token: HANDLE = ptr::null_mut();
    // SAFETY: `token` receives a handle closed below.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut len = 0;
    // SAFETY: a first call with no buffer answers the length it needs.
    unsafe { GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut len) };
    let mut buf = vec![0u8; len as usize];
    // SAFETY: `buf` holds `len` bytes.
    let filled =
        unsafe { GetTokenInformation(token, TokenUser, buf.as_mut_ptr().cast(), len, &mut len) };
    let result = match filled {
        0 => Err(io::Error::last_os_error()),
        _ => Ok(User(buf)),
    };
    unsafe { CloseHandle(token) };
    result
}

/// A security descriptor Windows allocated, freed once dropped.
struct Descriptor(PSECURITY_DESCRIPTOR);

impl Descriptor {
    fn from_sddl(sddl: &str) -> io::Result<Self> {
        let wide = wide(OsStr::new(sddl));
        let mut descriptor = ptr::null_mut();
        // SAFETY: `wide` is NUL-terminated; `descriptor` receives a buffer
        // the drop frees.
        let made = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            )
        };
        match made {
            0 => Err(io::Error::last_os_error()),
            _ => Ok(Self(descriptor)),
        }
    }
}

impl Drop for Descriptor {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: Windows allocated the descriptor with `LocalAlloc`.
            unsafe { LocalFree(self.0) };
        }
    }
}

fn wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain(Some(0)).collect()
}
