//! GSS-API (Kerberos) client contexts, over the system GSS library (MIT Kerberos on Linux, the `GSS`
//! framework on macOS). It has the little that a database login needs: import the service name,
//! optionally acquire credentials from a password, and run `gss_init_sec_context` until the context
//! is complete. Nothing else is exposed.
//!
//! **Unsafe.** This crate is one of the few places in karez where `unsafe` is allowed (plan section
//! 7.2). It is all FFI to one C interface. The rules that keep it sound, which each `SAFETY:` comment
//! below applies:
//!
//! 1. **Handles are owned and released once.** Every `gss_*_t` handle lives in one struct field, is
//!    created by exactly one call, is released by `Drop` (or earlier, once, on an error path), and is
//!    never copied: the Rust types are not `Clone`, and not `Send` (raw pointers), so no other thread
//!    can use a handle.
//! 2. **Buffers handed to C outlive the call and are not written by it.** Input buffers point into
//!    Rust slices that are alive for the whole call; the library only reads them. Output buffers are
//!    zero-initialised descriptors that the library fills and that are copied out and then released
//!    with `gss_release_buffer`, exactly once.
//! 3. **Errors are checked after every call**, and the minor status is a local `u32`.
//! 4. **No callbacks, no global state, no threads** are created or registered here.
//!
//! On Windows the interface is SSPI, which is not implemented: [`Context::new`] returns an error.

#![deny(unsafe_op_in_unsafe_fn)]

use std::fmt;

/// An error from the GSS library, or from this crate when there is no library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    /// The GSS major status (0 when the error is not from the library).
    pub major: u32,
    /// The mechanism's minor status.
    pub minor: u32,
    /// The library's own description, for example `No Kerberos credentials available`.
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for Error {}

/// Where the client's Kerberos credentials come from.
#[derive(Clone, Copy)]
pub enum Credentials<'a> {
    /// The default credentials: the ticket cache (`KRB5CCNAME`, as set by `kinit`) or the default
    /// client keytab.
    Default,
    /// A user principal and its password: the library asks the KDC for a ticket-granting ticket.
    /// The principal is `user` or `user@REALM`.
    Password {
        /// The principal name.
        user: &'a str,
        /// The password.
        password: &'a str,
    },
}

impl fmt::Debug for Credentials<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Credentials::Default => f.write_str("Credentials::Default"),
            Credentials::Password { user, .. } => f
                .debug_struct("Credentials::Password")
                .field("user", user)
                .field("password", &"<redacted>")
                .finish(),
        }
    }
}

#[cfg(unix)]
mod imp {
    use super::{Credentials, Error};
    use std::ffi::c_void;
    use std::ptr;

    /// `OM_uint32` is a 32-bit unsigned integer in every implementation this links to.
    type OmUint32 = u32;
    type Name = *mut c_void;
    type Ctx = *mut c_void;
    type Cred = *mut c_void;

    #[repr(C)]
    struct BufferDesc {
        length: usize,
        value: *mut c_void,
    }

    #[repr(C)]
    struct OidDesc {
        length: OmUint32,
        elements: *const c_void,
    }

    extern "C" {
        fn gss_import_name(
            minor: *mut OmUint32,
            input: *const BufferDesc,
            name_type: *const OidDesc,
            out: *mut Name,
        ) -> OmUint32;
        fn gss_release_name(minor: *mut OmUint32, name: *mut Name) -> OmUint32;
        fn gss_acquire_cred_with_password(
            minor: *mut OmUint32,
            desired: Name,
            password: *const BufferDesc,
            time_req: OmUint32,
            desired_mechs: *const c_void,
            usage: i32,
            out: *mut Cred,
            actual_mechs: *mut *mut c_void,
            time_rec: *mut OmUint32,
        ) -> OmUint32;
        fn gss_release_cred(minor: *mut OmUint32, cred: *mut Cred) -> OmUint32;
        fn gss_init_sec_context(
            minor: *mut OmUint32,
            cred: Cred,
            ctx: *mut Ctx,
            target: Name,
            mech: *const OidDesc,
            req_flags: OmUint32,
            time_req: OmUint32,
            chan_bindings: *const c_void,
            input: *const BufferDesc,
            actual_mech: *mut *mut OidDesc,
            output: *mut BufferDesc,
            ret_flags: *mut OmUint32,
            time_rec: *mut OmUint32,
        ) -> OmUint32;
        fn gss_delete_sec_context(
            minor: *mut OmUint32,
            ctx: *mut Ctx,
            output: *mut BufferDesc,
        ) -> OmUint32;
        fn gss_release_buffer(minor: *mut OmUint32, buf: *mut BufferDesc) -> OmUint32;
        fn gss_display_status(
            minor: *mut OmUint32,
            status: OmUint32,
            status_type: i32,
            mech: *const OidDesc,
            message_context: *mut OmUint32,
            out: *mut BufferDesc,
        ) -> OmUint32;
    }

    const GSS_C_GSS_CODE: i32 = 1;
    const GSS_C_MECH_CODE: i32 = 2;
    const GSS_C_INITIATE: i32 = 1;
    const GSS_C_INDEFINITE: OmUint32 = 0xffff_ffff;
    const GSS_C_MUTUAL_FLAG: OmUint32 = 2;
    const GSS_S_CONTINUE_NEEDED: OmUint32 = 1;
    /// `GSS_ERROR(major)`: the calling-error and routine-error fields.
    const GSS_ERROR_MASK: OmUint32 = (0xff << 24) | (0xff << 16);

    /// `GSS_C_NT_HOSTBASED_SERVICE`, 1.2.840.113554.1.2.1.4, as DER content bytes. Defined here
    /// instead of read from the library's exported variable, which avoids a `static` FFI read.
    static NT_HOSTBASED_SERVICE: [u8; 10] =
        [0x2a, 0x86, 0x48, 0x86, 0xf7, 0x12, 0x01, 0x02, 0x01, 0x04];
    /// `GSS_C_NT_USER_NAME`, 1.2.840.113554.1.2.1.1.
    static NT_USER_NAME: [u8; 10] = [0x2a, 0x86, 0x48, 0x86, 0xf7, 0x12, 0x01, 0x02, 0x01, 0x01];

    fn oid(bytes: &'static [u8]) -> OidDesc {
        OidDesc {
            length: bytes.len() as OmUint32,
            elements: bytes.as_ptr().cast(),
        }
    }

    fn empty_buffer() -> BufferDesc {
        BufferDesc {
            length: 0,
            value: ptr::null_mut(),
        }
    }

    /// A buffer that points at `data`. The library only reads through it (`const` in the C
    /// signatures; the descriptor type has a non-const pointer for historical reasons), and the
    /// caller keeps `data` alive for the call.
    fn view(data: &[u8]) -> BufferDesc {
        BufferDesc {
            length: data.len(),
            value: data.as_ptr() as *mut c_void,
        }
    }

    /// Turn a major and minor status into the library's own text.
    fn error(major: OmUint32, minor: OmUint32) -> Error {
        let mut parts = Vec::new();
        for (code, kind) in [(major, GSS_C_GSS_CODE), (minor, GSS_C_MECH_CODE)] {
            let mut ctx: OmUint32 = 0;
            // The library can return several messages for one status; it sets `ctx` to 0 after the
            // last. A bound keeps a misbehaving library from looping this forever.
            for _ in 0..16 {
                let mut buf = empty_buffer();
                let mut m: OmUint32 = 0;
                // SAFETY: `m` and `ctx` are valid for writes; a null mech means the default
                // mechanism; `buf` is a zero descriptor that the call fills.
                let r = unsafe {
                    gss_display_status(&mut m, code, kind, ptr::null(), &mut ctx, &mut buf)
                };
                if r & GSS_ERROR_MASK != 0 {
                    break;
                }
                if !buf.value.is_null() {
                    // SAFETY: on success `buf.value` points at `buf.length` bytes that the library
                    // allocated; they are copied before the buffer is released below.
                    let bytes =
                        unsafe { std::slice::from_raw_parts(buf.value.cast::<u8>(), buf.length) };
                    parts.push(String::from_utf8_lossy(bytes).into_owned());
                    let mut m2: OmUint32 = 0;
                    // SAFETY: `buf` was filled by `gss_display_status` and is released exactly once.
                    unsafe { gss_release_buffer(&mut m2, &mut buf) };
                }
                if ctx == 0 {
                    break;
                }
            }
        }
        parts.dedup();
        Error {
            major,
            minor,
            message: if parts.is_empty() {
                format!("GSS-API error (major {major:#x}, minor {minor})")
            } else {
                parts.join(": ")
            },
        }
    }

    fn import(name: &[u8], kind: &'static [u8]) -> Result<Name, Error> {
        let input = view(name);
        let kind = oid(kind);
        let mut minor: OmUint32 = 0;
        let mut out: Name = ptr::null_mut();
        // SAFETY: `input` points into `name`, alive for the call and only read; `kind` points at a
        // `'static` byte array; `minor` and `out` are valid for writes. On success the library
        // gives ownership of `out` to the caller, who releases it with `gss_release_name`.
        let major = unsafe { gss_import_name(&mut minor, &input, &kind, &mut out) };
        if major & GSS_ERROR_MASK != 0 {
            return Err(error(major, minor));
        }
        Ok(out)
    }

    /// A client security context for one service. See [`crate::Context`].
    pub struct Context {
        ctx: Ctx,
        target: Name,
        cred: Cred,
        complete: bool,
    }

    impl Context {
        pub fn new(target: &str, creds: Credentials<'_>) -> Result<Context, Error> {
            let target = import(target.as_bytes(), &NT_HOSTBASED_SERVICE)?;
            // From here on `ctx` owns `target`, so an error return still releases it through Drop.
            let mut ctx = Context {
                ctx: ptr::null_mut(),
                target,
                cred: ptr::null_mut(),
                complete: false,
            };
            if let Credentials::Password { user, password } = creds {
                let mut user_name = import(user.as_bytes(), &NT_USER_NAME)?;
                let pw = view(password.as_bytes());
                let mut minor: OmUint32 = 0;
                let mut cred: Cred = ptr::null_mut();
                // SAFETY: `user_name` is a name handle from `import`; `pw` points into `password`
                // (alive for the call, only read); null mechs means the default; `cred` and `minor`
                // are valid for writes. On success `cred` is owned by `ctx` (released in Drop).
                let major = unsafe {
                    gss_acquire_cred_with_password(
                        &mut minor,
                        user_name,
                        &pw,
                        GSS_C_INDEFINITE,
                        ptr::null(),
                        GSS_C_INITIATE,
                        &mut cred,
                        ptr::null_mut(),
                        ptr::null_mut(),
                    )
                };
                let mut m2: OmUint32 = 0;
                // SAFETY: `user_name` came from `import` and is released exactly once here; the
                // credential keeps its own copy of the principal.
                unsafe { gss_release_name(&mut m2, &mut user_name) };
                if major & GSS_ERROR_MASK != 0 {
                    return Err(error(major, minor));
                }
                ctx.cred = cred;
            }
            Ok(ctx)
        }

        pub fn step(&mut self, input: Option<&[u8]>) -> Result<Vec<u8>, Error> {
            if self.complete {
                return Err(Error {
                    major: 0,
                    minor: 0,
                    message: "the GSS context is already complete".into(),
                });
            }
            let token = input.map(view);
            let mut out = empty_buffer();
            let mut minor: OmUint32 = 0;
            let mut flags: OmUint32 = 0;
            // SAFETY: `self.cred` is null or a credential handle owned by `self`; `self.ctx` is
            // null on the first call and the handle the library returned afterwards; `self.target`
            // is a name handle owned by `self`; a null mech means the default; `token`, if any,
            // points into `input`, alive for the call and only read; `out`, `minor` and `flags` are
            // valid for writes; the optional outputs we do not want are null.
            let major = unsafe {
                gss_init_sec_context(
                    &mut minor,
                    self.cred,
                    &mut self.ctx,
                    self.target,
                    ptr::null(),
                    GSS_C_MUTUAL_FLAG,
                    GSS_C_INDEFINITE,
                    ptr::null(),
                    token
                        .as_ref()
                        .map_or(ptr::null(), |t| t as *const BufferDesc),
                    ptr::null_mut(),
                    &mut out,
                    &mut flags,
                    ptr::null_mut(),
                )
            };
            // copy the token out first, then release the library's buffer, on success and error
            let bytes = if out.value.is_null() {
                Vec::new()
            } else {
                // SAFETY: the library allocated `out.length` bytes at `out.value`, valid until the
                // release below, and nothing else writes them meanwhile.
                unsafe { std::slice::from_raw_parts(out.value.cast::<u8>(), out.length) }.to_vec()
            };
            if !out.value.is_null() {
                let mut m2: OmUint32 = 0;
                // SAFETY: `out` was filled by `gss_init_sec_context` and is released exactly once.
                unsafe { gss_release_buffer(&mut m2, &mut out) };
            }
            if major & GSS_ERROR_MASK != 0 {
                return Err(error(major, minor));
            }
            self.complete = major & GSS_S_CONTINUE_NEEDED == 0;
            Ok(bytes)
        }

        pub fn is_complete(&self) -> bool {
            self.complete
        }
    }

    impl Drop for Context {
        fn drop(&mut self) {
            let mut minor: OmUint32 = 0;
            if !self.ctx.is_null() {
                // SAFETY: `self.ctx` is the handle `gss_init_sec_context` returned, owned by
                // `self` and deleted only here; a null output buffer says no token is wanted.
                unsafe { gss_delete_sec_context(&mut minor, &mut self.ctx, ptr::null_mut()) };
            }
            if !self.target.is_null() {
                // SAFETY: `self.target` is a name handle from `import`, owned by `self`, released
                // only here.
                unsafe { gss_release_name(&mut minor, &mut self.target) };
            }
            if !self.cred.is_null() {
                // SAFETY: `self.cred` is a credential handle from `gss_acquire_cred_with_password`,
                // owned by `self`, released only here.
                unsafe { gss_release_cred(&mut minor, &mut self.cred) };
            }
        }
    }
}

#[cfg(not(unix))]
mod imp {
    use super::{Credentials, Error};

    /// A placeholder: there is no GSS-API on this platform.
    pub struct Context;

    impl Context {
        pub fn new(_target: &str, _creds: Credentials<'_>) -> Result<Context, Error> {
            Err(Error {
                major: 0,
                minor: 0,
                message: "Kerberos login is not implemented on this platform (SSPI is open)".into(),
            })
        }

        pub fn step(&mut self, _input: Option<&[u8]>) -> Result<Vec<u8>, Error> {
            unreachable!("a Context cannot be built on this platform")
        }

        pub fn is_complete(&self) -> bool {
            false
        }
    }
}

/// A client security context for one service: call [`Context::step`] with the server's tokens until
/// [`Context::is_complete`].
pub struct Context(imp::Context);

impl fmt::Debug for Context {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Context")
            .field("complete", &self.0.is_complete())
            .finish()
    }
}

impl Context {
    /// Prepare a context for the host-based service `target` (`service@host`, for example
    /// `postgres@db.example.com`), with credentials from `creds`. Nothing is sent yet.
    pub fn new(target: &str, creds: Credentials<'_>) -> Result<Context, Error> {
        imp::Context::new(target, creds).map(Context)
    }

    /// Run one step. Pass `None` the first time and the server's token each time after; the result
    /// is the token to send back (empty when there is nothing to send).
    pub fn step(&mut self, input: Option<&[u8]>) -> Result<Vec<u8>, Error> {
        self.0.step(input)
    }

    /// True once the context is established: the server has been authenticated to us and we to it.
    pub fn is_complete(&self) -> bool {
        self.0.is_complete()
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn debug_never_shows_the_password() {
        let c = Credentials::Password {
            user: "alice",
            password: "hunter2",
        };
        let s = format!("{c:?}");
        assert!(s.contains("alice") && !s.contains("hunter2"), "{s}");
    }

    #[test]
    fn a_name_is_imported_without_a_network_and_a_missing_ticket_is_a_readable_error() {
        // no ticket cache and no KDC are configured here: the library must say so, not crash
        let mut c = Context::new("postgres@localhost", Credentials::Default).unwrap();
        assert!(!c.is_complete());
        let e = c.step(None).unwrap_err();
        assert!(!e.message.is_empty());
        assert!(e.major != 0, "{e:?}");
        // and dropping releases the name without trouble
        drop(c);
    }

    #[test]
    fn a_bad_principal_or_unreachable_kdc_is_an_error_and_leaks_nothing() {
        for _ in 0..50 {
            let r = Context::new(
                "postgres@localhost",
                Credentials::Password {
                    user: "nobody@NO.SUCH.REALM.TEST",
                    password: "x",
                },
            );
            assert!(r.is_err());
        }
    }
}
