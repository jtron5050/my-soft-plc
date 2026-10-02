//! GPIO character-device uAPI v2. Unsafe is confined to this module.
//!
//! Layout matches `linux/gpio.h`: `gpio_v2_line_values` 16, attribute 16,
//! config attribute 24, `gpio_v2_line_config` 272, `gpio_v2_line_request` 592.

#![allow(unsafe_code)]

use std::ffi::CString;
use std::io::Error;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::Path;

use plc_io::IoError;

use crate::validate::{Bias, Drive};

/// One `GPIO_V2_GET_LINE` request's worth of lines.
pub(crate) struct LineClaim {
    /// `/dev/gpiochipN`.
    pub chip_path: std::path::PathBuf,
    /// Offsets in request order.
    pub offsets: Vec<u32>,
    /// `gpio_v2_line_flag` bits.
    pub flags: u64,
    /// Initial output bitmap. Ignored for inputs.
    pub initial_values: u64,
    /// Set values on release when true.
    pub is_output: bool,
    /// Bits that this request owns.
    pub mask: u64,
}

pub(crate) trait LineIo: Send {
    /// Read the module bitmap. Bit *i* is `offsets[i]`.
    fn read_bits(&mut self, module: usize) -> Result<u64, ()>;

    /// Write `bits` where `mask` is set.
    fn write_bits(&mut self, module: usize, bits: u64, mask: u64) -> Result<(), ()>;

    /// Drive `safe_bits` on outputs, then close the request fd.
    fn release(&mut self, module: usize, safe_bits: u64);
}

const FLAG_ACTIVE_LOW: u64 = 1 << 1;
const FLAG_INPUT: u64 = 1 << 2;
const FLAG_OUTPUT: u64 = 1 << 3;
const FLAG_OPEN_DRAIN: u64 = 1 << 6;
const FLAG_BIAS_PULL_UP: u64 = 1 << 8;
const FLAG_BIAS_PULL_DOWN: u64 = 1 << 9;
const FLAG_BIAS_DISABLED: u64 = 1 << 10;

const ATTR_OUTPUT_VALUES: u32 = 2;

const GPIO_V2_GET_LINE_IOCTL: u64 = 0xc250_b407;
const GPIO_V2_LINE_GET_VALUES_IOCTL: u64 = 0xc010_b40e;
const GPIO_V2_LINE_SET_VALUES_IOCTL: u64 = 0xc010_b40f;

const LINES_MAX: usize = 64;

/// Flags stored in `gpio_v2_line_config.flags`.
pub(crate) fn config_flags(
    is_input: bool,
    active_low: bool,
    bias: Bias,
    drive: Option<Drive>,
) -> u64 {
    let mut flags = if is_input { FLAG_INPUT } else { FLAG_OUTPUT };
    if active_low {
        flags |= FLAG_ACTIVE_LOW;
    }
    if drive == Some(Drive::OpenDrain) {
        flags |= FLAG_OPEN_DRAIN;
    }
    flags |= match bias {
        Bias::AsIs => 0,
        Bias::PullUp => FLAG_BIAS_PULL_UP,
        Bias::PullDown => FLAG_BIAS_PULL_DOWN,
        Bias::Disabled => FLAG_BIAS_DISABLED,
    };
    flags
}

/// Bitmap covering the first `n` requested lines. `n == 64` is all bits.
pub(crate) fn line_mask(n: usize) -> u64 {
    if n >= 64 {
        u64::MAX
    } else {
        (1_u64 << n) - 1
    }
}

struct LineReq {
    fd: Option<OwnedFd>,
    is_output: bool,
    mask: u64,
}

pub(crate) struct LinuxLines {
    reqs: Vec<LineReq>,
}

impl LinuxLines {
    /// Request every module. On failure, release anything already claimed.
    pub(crate) fn claim(claims: &[LineClaim]) -> Result<Self, IoError> {
        let mut lines = Self {
            reqs: Vec::with_capacity(claims.len()),
        };
        for claim in claims {
            match request_one(claim) {
                Ok(fd) => lines.reqs.push(LineReq {
                    fd: Some(fd),
                    is_output: claim.is_output,
                    mask: claim.mask,
                }),
                Err(err) => {
                    for (index, req) in lines.reqs.iter_mut().enumerate() {
                        let safe = if claims[index].is_output {
                            claims[index].initial_values
                        } else {
                            0
                        };
                        release_req(req, safe);
                    }
                    return Err(err);
                }
            }
        }
        Ok(lines)
    }
}

impl LineIo for LinuxLines {
    fn read_bits(&mut self, module: usize) -> Result<u64, ()> {
        let Some(req) = self.reqs.get_mut(module) else {
            return Err(());
        };
        let mask = req.mask;
        let Some(fd) = req.fd.as_ref() else {
            return Err(());
        };
        let mut values = GpioV2LineValues { bits: 0, mask };
        ioctl_values(fd.as_raw_fd(), GPIO_V2_LINE_GET_VALUES_IOCTL, &mut values)?;
        Ok(values.bits)
    }

    fn write_bits(&mut self, module: usize, bits: u64, mask: u64) -> Result<(), ()> {
        let Some(req) = self.reqs.get_mut(module) else {
            return Err(());
        };
        if !req.is_output {
            return Err(());
        }
        let Some(fd) = req.fd.as_ref() else {
            return Err(());
        };
        let mut values = GpioV2LineValues { bits, mask };
        ioctl_values(fd.as_raw_fd(), GPIO_V2_LINE_SET_VALUES_IOCTL, &mut values)
    }

    fn release(&mut self, module: usize, safe_bits: u64) {
        if let Some(req) = self.reqs.get_mut(module) {
            release_req(req, safe_bits);
        }
    }
}

fn release_req(req: &mut LineReq, safe_bits: u64) {
    if req.is_output {
        let mask = req.mask;
        if let Some(fd) = req.fd.as_ref() {
            let mut values = GpioV2LineValues {
                bits: safe_bits,
                mask,
            };
            let _ = ioctl_values(fd.as_raw_fd(), GPIO_V2_LINE_SET_VALUES_IOCTL, &mut values);
        }
    }
    req.fd.take();
}

fn request_one(claim: &LineClaim) -> Result<OwnedFd, IoError> {
    let label = chip_label(&claim.chip_path);
    let chip = open_chip(&claim.chip_path, label)?;
    let mut req = GpioV2LineRequest::default();
    let count = claim.offsets.len().min(LINES_MAX);
    req.num_lines = u32::try_from(count).unwrap_or(0);
    for (index, offset) in claim.offsets.iter().take(count).enumerate() {
        req.offsets[index] = *offset;
    }
    let name = b"soft-plc";
    req.consumer[..name.len()].copy_from_slice(name);
    req.config.flags = claim.flags;
    if claim.is_output {
        req.config.num_attrs = 1;
        req.config.attrs[0].attr.id = ATTR_OUTPUT_VALUES;
        req.config.attrs[0].attr.value = claim.initial_values;
        req.config.attrs[0].mask = claim.mask;
    }
    let rc = unsafe {
        libc::ioctl(
            chip.as_raw_fd(),
            GPIO_V2_GET_LINE_IOCTL as libc::Ioctl,
            &mut req,
        )
    };
    if rc < 0 {
        let err = Error::last_os_error();
        return Err(IoError::Driver(format_line_error(label, &err)));
    }
    if req.fd < 0 {
        return Err(IoError::Driver(format!(
            "{label}: gpiochip returned no request fd"
        )));
    }
    // SAFETY: the kernel wrote a new request fd into `req.fd`. This owner closes it.
    Ok(unsafe { OwnedFd::from_raw_fd(req.fd) })
}

fn open_chip(path: &Path, label: &str) -> Result<OwnedFd, IoError> {
    let text = path.to_string_lossy();
    let c_path = CString::new(text.as_ref())
        .map_err(|_| IoError::Driver(format!("{label}: chip path contains a nul")))?;
    let fd = unsafe { libc::open(c_path.as_ptr(), libc::O_RDWR | libc::O_CLOEXEC) };
    if fd < 0 {
        let err = Error::last_os_error();
        return Err(IoError::Driver(format_line_error(label, &err)));
    }
    // SAFETY: `fd` is a fresh descriptor from `open`. This owner closes it.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn ioctl_values(fd: i32, request: u64, values: &mut GpioV2LineValues) -> Result<(), ()> {
    let rc = unsafe { libc::ioctl(fd, request as libc::Ioctl, values) };
    if rc < 0 {
        Err(())
    } else {
        Ok(())
    }
}

fn chip_label(path: &Path) -> &str {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("gpiochip")
}

fn format_line_error(chip: &str, err: &Error) -> String {
    if err.raw_os_error() == Some(libc::ENOTTY) {
        format!("{chip}: {err} (not a gpiochip or kernel lacks GPIO uAPI v2)")
    } else {
        format!("{chip}: {err}")
    }
}

#[repr(C)]
struct GpioV2LineValues {
    bits: u64,
    mask: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct GpioV2LineAttribute {
    id: u32,
    padding: u32,
    value: u64,
}

impl Default for GpioV2LineAttribute {
    fn default() -> Self {
        Self {
            id: 0,
            padding: 0,
            value: 0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct GpioV2LineConfigAttribute {
    attr: GpioV2LineAttribute,
    mask: u64,
}

impl Default for GpioV2LineConfigAttribute {
    fn default() -> Self {
        Self {
            attr: GpioV2LineAttribute::default(),
            mask: 0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct GpioV2LineConfig {
    flags: u64,
    num_attrs: u32,
    padding: [u32; 5],
    attrs: [GpioV2LineConfigAttribute; 10],
}

impl Default for GpioV2LineConfig {
    fn default() -> Self {
        Self {
            flags: 0,
            num_attrs: 0,
            padding: [0; 5],
            attrs: [GpioV2LineConfigAttribute::default(); 10],
        }
    }
}

#[repr(C)]
struct GpioV2LineRequest {
    offsets: [u32; LINES_MAX],
    consumer: [u8; 32],
    config: GpioV2LineConfig,
    num_lines: u32,
    event_buffer_size: u32,
    padding: [u32; 5],
    fd: i32,
}

impl Default for GpioV2LineRequest {
    fn default() -> Self {
        Self {
            offsets: [0; LINES_MAX],
            consumer: [0; 32],
            config: GpioV2LineConfig::default(),
            num_lines: 0,
            event_buffer_size: 0,
            padding: [0; 5],
            fd: -1,
        }
    }
}

const _: () = assert!(std::mem::size_of::<GpioV2LineValues>() == 16);
const _: () = assert!(std::mem::size_of::<GpioV2LineAttribute>() == 16);
const _: () = assert!(std::mem::size_of::<GpioV2LineConfigAttribute>() == 24);
const _: () = assert!(std::mem::size_of::<GpioV2LineConfig>() == 272);
const _: () = assert!(std::mem::size_of::<GpioV2LineRequest>() == 592);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_bits_match_uapi() {
        let open = config_flags(false, false, Bias::AsIs, Some(Drive::OpenDrain));
        assert_eq!(open & FLAG_OUTPUT, FLAG_OUTPUT);
        assert_eq!(open & FLAG_OPEN_DRAIN, FLAG_OPEN_DRAIN);
        let push = config_flags(false, false, Bias::AsIs, Some(Drive::PushPull));
        assert_eq!(push & FLAG_OPEN_DRAIN, 0);
        assert_eq!(push & FLAG_OUTPUT, FLAG_OUTPUT);
        let input = config_flags(true, true, Bias::PullUp, None);
        assert_eq!(input & FLAG_INPUT, FLAG_INPUT);
        assert_eq!(input & FLAG_ACTIVE_LOW, FLAG_ACTIVE_LOW);
        assert_eq!(input & FLAG_BIAS_PULL_UP, FLAG_BIAS_PULL_UP);
        assert_eq!(input & FLAG_OPEN_DRAIN, 0);
        assert_eq!(line_mask(0), 0);
        assert_eq!(line_mask(3), 0b111);
        assert_eq!(line_mask(64), u64::MAX);
    }

    #[test]
    fn null_get_line_is_enotty() {
        let fd = unsafe { libc::open(c"/dev/null".as_ptr(), libc::O_RDWR | libc::O_CLOEXEC) };
        assert!(fd >= 0, "open /dev/null");
        let owned = unsafe { OwnedFd::from_raw_fd(fd) };
        let mut req = GpioV2LineRequest {
            num_lines: 1,
            config: GpioV2LineConfig {
                flags: FLAG_INPUT,
                ..GpioV2LineConfig::default()
            },
            ..GpioV2LineRequest::default()
        };
        let rc = unsafe {
            libc::ioctl(
                owned.as_raw_fd(),
                GPIO_V2_GET_LINE_IOCTL as libc::Ioctl,
                &mut req,
            )
        };
        assert!(rc < 0);
        let err = Error::last_os_error();
        assert_eq!(err.raw_os_error(), Some(libc::ENOTTY));
        let text = format_line_error("gpiochip0", &err);
        assert!(text.contains("gpiochip0"), "{text}");
        assert!(text.contains("uAPI v2"), "{text}");
    }

    #[test]
    fn sizes_match_system_header_when_cc_present() {
        if !Path::new("/usr/include/linux/gpio.h").exists() {
            return;
        }
        let src = std::env::temp_dir().join(format!("plc-io-gpio-uapi-{}.c", std::process::id()));
        let bin = std::env::temp_dir().join(format!("plc-io-gpio-uapi-{}", std::process::id()));
        let c_src = r#"
#include <linux/gpio.h>
#include <stdio.h>
int main(void) {
  printf("%zu %zu %zu %zu %zu\n",
    sizeof(struct gpio_v2_line_values),
    sizeof(struct gpio_v2_line_attribute),
    sizeof(struct gpio_v2_line_config_attribute),
    sizeof(struct gpio_v2_line_config),
    sizeof(struct gpio_v2_line_request));
  printf("0x%lx 0x%lx 0x%lx\n",
    (unsigned long)GPIO_V2_GET_LINE_IOCTL,
    (unsigned long)GPIO_V2_LINE_GET_VALUES_IOCTL,
    (unsigned long)GPIO_V2_LINE_SET_VALUES_IOCTL);
  return 0;
}
"#;
        std::fs::write(&src, c_src).expect("write c");
        let compile = std::process::Command::new("cc")
            .arg(&src)
            .arg("-o")
            .arg(&bin)
            .output();
        let Ok(compile) = compile else {
            let _ = std::fs::remove_file(&src);
            return;
        };
        assert!(
            compile.status.success(),
            "cc failed: {}",
            String::from_utf8_lossy(&compile.stderr)
        );
        let output = std::process::Command::new(&bin).output().expect("run");
        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&bin);
        assert!(output.status.success());
        let text = String::from_utf8(output.stdout).expect("utf8");
        let mut lines = text.lines();
        assert_eq!(lines.next(), Some("16 16 24 272 592"));
        assert_eq!(lines.next(), Some("0xc250b407 0xc010b40e 0xc010b40f"));
    }
}
