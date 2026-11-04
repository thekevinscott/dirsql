//! Which files a query must read again so its rows match the files on disk.
//!
//! A file's change time moves on every write and cannot be set back by
//! `utime`, so once it sits [`SETTLE_NS`] before the moment a read began, the
//! read saw the file's final content and an identical stat later proves the
//! file unchanged. Until then the file is read on every query.

use std::path::Path;

/// How far before a read's start a file's change time must sit for that read
/// to have seen its final content. Covers kernels whose timestamps lag the
/// clock by a tick and filesystems that keep whole seconds.
pub(crate) const SETTLE_NS: i64 = 2_000_000_000;

/// The stat of a file, following symlinks: what a change to it must alter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Fingerprint {
    pub(crate) dev: u64,
    pub(crate) ino: u64,
    pub(crate) size: u64,
    pub(crate) mtime_ns: i64,
    /// `None` where the platform reports no change time: such a file never
    /// settles, so every query reads it.
    pub(crate) ctime_ns: Option<i64>,
}

/// What the index holds for one `(table, file)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Known {
    /// Indexed by the build and not stat'd since.
    Built,
    /// Last read with the file at `fp` (`None`: it could not be stat'd).
    Read {
        fp: Option<Fingerprint>,
        settled: bool,
    },
}

/// Whether a file a query found must be read for its rows to be current,
/// given what the index holds for it and its stat now.
pub(crate) fn must_read(known: Option<&Known>, now: Option<&Fingerprint>, built_ns: i64) -> bool {
    match (known, now) {
        (None, _) | (_, None) => true,
        (Some(Known::Built), Some(now)) => !changed_before(now, built_ns),
        (Some(Known::Read { fp, settled }), Some(now)) => !settled || fp.as_ref() != Some(now),
    }
}

/// Whether `fp` last changed far enough before `start_ns` that a read begun
/// then saw its final content.
pub(crate) fn changed_before(fp: &Fingerprint, start_ns: i64) -> bool {
    fp.ctime_ns
        .is_some_and(|ctime| ctime < start_ns.saturating_sub(SETTLE_NS))
}

/// What the index records for a file read at `start_ns` and stat'd after.
pub(crate) fn after_read(fp: Option<Fingerprint>, start_ns: i64) -> Known {
    Known::Read {
        fp,
        settled: fp.is_some_and(|fp| changed_before(&fp, start_ns)),
    }
}

/// What the index records for a file a query need not read: a built file
/// counts as read when the build began; any other entry stands.
pub(crate) fn adopted(
    known: Option<&Known>,
    now: Option<Fingerprint>,
    built_ns: i64,
) -> Option<Known> {
    (known == Some(&Known::Built)).then(|| after_read(now, built_ns))
}

#[cfg(unix)]
pub(crate) fn fingerprint(path: &Path) -> std::io::Result<Fingerprint> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(path)?;
    Ok(Fingerprint {
        dev: meta.dev(),
        ino: meta.ino(),
        size: meta.size(),
        mtime_ns: unix_ns(meta.mtime(), meta.mtime_nsec()),
        ctime_ns: Some(unix_ns(meta.ctime(), meta.ctime_nsec())),
    })
}

#[cfg(unix)]
fn unix_ns(secs: i64, nanos: i64) -> i64 {
    secs.saturating_mul(1_000_000_000).saturating_add(nanos)
}

/// Windows keeps the change time (NTFS `ChangeTime`) out of `std`'s stable
/// metadata, so it is read from the handle, with the volume serial and file
/// index standing in for `dev` and `ino`.
#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "GetFileInformationByHandle(Ex) are the only way to read ChangeTime and the file index"
)]
pub(crate) fn fingerprint(path: &Path) -> std::io::Result<Fingerprint> {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_BASIC_INFO, FILE_READ_ATTRIBUTES, FileBasicInfo,
        GetFileInformationByHandle, GetFileInformationByHandleEx,
    };
    let file = std::fs::OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .open(path)?;
    let handle = file.as_raw_handle();
    // SAFETY: both structs are plain data for which all-zero is valid, and
    // each call writes at most the size it is given into its own struct
    // through a handle `file` keeps open.
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetFileInformationByHandle(handle, &mut info) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut basic: FILE_BASIC_INFO = unsafe { std::mem::zeroed() };
    let size = u32::try_from(std::mem::size_of::<FILE_BASIC_INFO>()).expect("small struct");
    if unsafe { GetFileInformationByHandleEx(handle, FileBasicInfo, (&raw mut basic).cast(), size) }
        == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(Fingerprint {
        dev: u64::from(info.dwVolumeSerialNumber),
        ino: (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
        size: (u64::from(info.nFileSizeHigh) << 32) | u64::from(info.nFileSizeLow),
        mtime_ns: windows_ns(basic.LastWriteTime),
        ctime_ns: (basic.ChangeTime != 0).then(|| windows_ns(basic.ChangeTime)),
    })
}

/// A Windows file time (100ns ticks since 1601) as nanoseconds since 1970.
#[cfg(windows)]
fn windows_ns(ticks: i64) -> i64 {
    ticks
        .saturating_sub(116_444_736_000_000_000)
        .saturating_mul(100)
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn fingerprint(path: &Path) -> std::io::Result<Fingerprint> {
    let meta = std::fs::metadata(path)?;
    Ok(Fingerprint {
        dev: 0,
        ino: 0,
        size: meta.len(),
        mtime_ns: 0,
        ctime_ns: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fp(ctime_ns: Option<i64>) -> Fingerprint {
        Fingerprint {
            dev: 1,
            ino: 2,
            size: 3,
            mtime_ns: 4,
            ctime_ns,
        }
    }

    const START: i64 = 10 * SETTLE_NS;

    #[test]
    fn a_change_time_settles_only_strictly_before_the_margin() {
        assert!(changed_before(&fp(Some(START - SETTLE_NS - 1)), START));
        assert!(!changed_before(&fp(Some(START - SETTLE_NS)), START));
        assert!(!changed_before(&fp(Some(START)), START));
    }

    #[test]
    fn a_built_file_is_adopted_as_read_when_the_build_began() {
        let now = fp(Some(START - SETTLE_NS - 1));
        assert_eq!(
            adopted(Some(&Known::Built), Some(now), START),
            Some(Known::Read {
                fp: Some(now),
                settled: true
            })
        );
    }

    #[test]
    fn a_file_already_read_or_unknown_is_not_adopted() {
        let read = Known::Read {
            fp: Some(fp(Some(0))),
            settled: true,
        };
        assert_eq!(adopted(Some(&read), Some(fp(Some(0))), START), None);
        assert_eq!(adopted(None, Some(fp(Some(0))), START), None);
    }

    #[test]
    fn a_file_without_a_change_time_never_settles() {
        assert!(!changed_before(&fp(None), START));
    }

    #[test]
    fn the_margin_does_not_overflow_at_the_earliest_time() {
        assert!(!changed_before(&fp(Some(i64::MIN)), i64::MIN));
    }

    #[test]
    fn a_file_the_index_has_never_held_is_read() {
        assert!(must_read(None, Some(&fp(Some(0))), START));
    }

    #[test]
    fn a_file_that_cannot_be_stat_ed_is_read() {
        let known = after_read(Some(fp(Some(0))), START);
        assert!(must_read(Some(&known), None, START));
    }

    #[test]
    fn a_built_file_is_read_unless_it_changed_before_the_build_began() {
        let old = fp(Some(START - SETTLE_NS - 1));
        let recent = fp(Some(START - SETTLE_NS));
        assert!(!must_read(Some(&Known::Built), Some(&old), START));
        assert!(must_read(Some(&Known::Built), Some(&recent), START));
    }

    #[test]
    fn a_settled_file_is_read_again_only_when_its_stat_moves() {
        let old = fp(Some(0));
        let known = after_read(Some(old), START);
        assert!(!must_read(Some(&known), Some(&old), START));
        let moved = Fingerprint { size: 9, ..old };
        assert!(must_read(Some(&known), Some(&moved), START));
    }

    #[test]
    fn an_unsettled_file_is_read_on_every_query() {
        let recent = fp(Some(START));
        let known = after_read(Some(recent), START);
        assert!(must_read(Some(&known), Some(&recent), START));
    }

    #[test]
    fn a_read_settles_a_file_only_when_its_change_time_is_old() {
        assert_eq!(
            after_read(Some(fp(Some(0))), START),
            Known::Read {
                fp: Some(fp(Some(0))),
                settled: true
            }
        );
        assert_eq!(
            after_read(Some(fp(Some(START))), START),
            Known::Read {
                fp: Some(fp(Some(START))),
                settled: false
            }
        );
        assert_eq!(
            after_read(None, START),
            Known::Read {
                fp: None,
                settled: false
            }
        );
    }
}
