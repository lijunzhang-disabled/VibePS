use crate::{audio, video};
use ps1_core::{
    cdrom::CdromSectorSize,
    test_runner::{
        PsxExeExitCodeSource, PsxExePassCondition, PsxExeRunReport, PsxExeStopCondition,
        PsxExeTestConfig,
    },
    Ps1,
};
use std::env;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
struct Args {
    bios: Option<PathBuf>,
    disc: Option<PathBuf>,
    disc_sector_size: Option<CdromSectorSize>,
    exe: Option<PathBuf>,
    trace: Option<PathBuf>,
    steps: u64,
    test_mode: bool,
    test_mailbox: Option<(u32, u32)>,
    test_stop_pc: Option<u32>,
    test_pass_reg: Option<(u8, u32)>,
    test_exit_reg: Option<u8>,
}

pub fn run() -> Result<(), String> {
    let args = parse_args()?;

    let bios = args.bios.as_ref().map(|path| {
        fs::read(path).unwrap_or_else(|err| {
            eprintln!("failed to read BIOS {}: {err}", path.display());
            std::process::exit(1);
        })
    });

    let mut ps1 = Ps1::new(bios);
    if let Some(path) = args.disc.as_ref() {
        let (disc, sector_size) = load_disc_image(path, args.disc_sector_size)?;
        ps1.bus
            .cdrom
            .load_disc_image(disc, sector_size)
            .map_err(|err| format!("failed to load disc {}: {err:?}", path.display()))?;
    }

    let exe_loaded = if let Some(path) = args.exe.as_ref() {
        let exe = fs::read(path)
            .map_err(|err| format!("failed to read EXE {}: {err}", path.display()))?;
        ps1.load_psx_exe(&exe)
            .map_err(|err| format!("failed to load EXE {}: {err:?}", path.display()))?;
        true
    } else {
        false
    };

    let steps = args.steps.max(1);
    if args.uses_test_runner() {
        if !exe_loaded {
            return Err("PS-EXE test mode requires --exe PATH".to_string());
        }
        let config = build_test_config(&args, steps);
        let report = ps1.run_loaded_psx_exe_test(&config);
        print_test_report(&report);
        if report.status.is_success() {
            return Ok(());
        }
        return Err(format!("PS-EXE test did not pass: {:?}", report.status));
    }

    let mut trace = match args.trace {
        Some(path) => Some(BufWriter::new(File::create(&path).map_err(|err| {
            format!("failed to create trace {}: {err}", path.display())
        })?)),
        None => None,
    };
    let mut cycles = 0u64;
    for step in 0..steps {
        if let Some(trace) = trace.as_mut() {
            write_trace_line(trace, step, &ps1)?;
        }
        cycles += ps1.step_one() as u64;
    }
    if let Some(trace) = trace.as_mut() {
        trace.flush().map_err(trace_write_error)?;
    }

    println!(
        "steps={steps} cycles={cycles} pc=0x{pc:08x} next_pc=0x{next:08x} r2=0x{r2:08x} r29=0x{sp:08x} r31=0x{ra:08x} istat=0x{istat:04x} imask=0x{imask:04x} {video} {audio}",
        pc = ps1.cpu.pc,
        next = ps1.cpu.next_pc,
        r2 = ps1.cpu.regs[2],
        sp = ps1.cpu.regs[29],
        ra = ps1.cpu.regs[31],
        istat = ps1.bus.irq.status(),
        imask = ps1.bus.irq.mask(),
        video = video::display_summary(&ps1),
        audio = audio::audio_summary(),
    );

    Ok(())
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        steps: 100_000,
        ..Args::default()
    };
    let mut iter = env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--bios" => {
                args.bios = Some(PathBuf::from(iter.next().ok_or("--bios requires a path")?));
            }
            "--disc" => {
                args.disc = Some(PathBuf::from(iter.next().ok_or("--disc requires a path")?));
            }
            "--disc-sector-size" => {
                let value = iter
                    .next()
                    .ok_or("--disc-sector-size requires 2048 or 2352")?;
                args.disc_sector_size = Some(parse_disc_sector_size(&value)?);
            }
            "--exe" => {
                args.exe = Some(PathBuf::from(iter.next().ok_or("--exe requires a path")?));
            }
            "--trace" => {
                args.trace = Some(PathBuf::from(iter.next().ok_or("--trace requires a path")?));
            }
            "--test" => {
                args.test_mode = true;
            }
            "--test-mailbox" => {
                let value = iter.next().ok_or("--test-mailbox requires ADDR=PASS")?;
                args.test_mailbox = Some(parse_addr_value(&value, "--test-mailbox")?);
            }
            "--test-stop-pc" => {
                let value = iter.next().ok_or("--test-stop-pc requires an address")?;
                args.test_stop_pc = Some(parse_u32(&value)?);
            }
            "--test-pass-reg" => {
                let value = iter.next().ok_or("--test-pass-reg requires REG=VALUE")?;
                args.test_pass_reg = Some(parse_reg_value(&value, "--test-pass-reg")?);
            }
            "--test-exit-reg" => {
                let value = iter.next().ok_or("--test-exit-reg requires a register")?;
                args.test_exit_reg = Some(parse_reg(&value)?);
            }
            "--steps" => {
                let value = iter.next().ok_or("--steps requires a value")?;
                args.steps = value
                    .parse()
                    .map_err(|_| format!("invalid --steps value: {value}"))?;
            }
            "--help" | "-h" => usage_and_exit(),
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(args)
}

impl Args {
    fn uses_test_runner(&self) -> bool {
        self.test_mode
            || self.test_mailbox.is_some()
            || self.test_stop_pc.is_some()
            || self.test_pass_reg.is_some()
            || self.test_exit_reg.is_some()
    }
}

fn build_test_config(args: &Args, steps: u64) -> PsxExeTestConfig {
    let mut config = PsxExeTestConfig::new().with_max_steps(steps);
    if let Some((addr, pass_value)) = args.test_mailbox {
        config = config.with_mailbox32(addr, pass_value);
    }
    if let Some(pc) = args.test_stop_pc {
        config = config.stop_when(PsxExeStopCondition::Pc(pc));
    }
    if let Some((reg, value)) = args.test_pass_reg {
        config = config.pass_when(PsxExePassCondition::RegisterEquals { reg, value });
    }
    if let Some(reg) = args.test_exit_reg {
        config = config.exit_code_from(PsxExeExitCodeSource::Register(reg));
    }
    config
}

fn print_test_report(report: &PsxExeRunReport) {
    println!(
        "test_status={:?} steps={} cycles={} pc=0x{:08x} next_pc=0x{:08x} stop={:?} exit_code={}",
        report.status,
        report.steps,
        report.cycles,
        report.pc,
        report.next_pc,
        report.stop_reason,
        report
            .exit_code
            .map(|code| format!("0x{code:08x}"))
            .unwrap_or_else(|| "none".to_string()),
    );
}

fn write_trace_line<W: Write>(writer: &mut W, step: u64, ps1: &Ps1) -> Result<(), String> {
    let opcode = ps1.bus.peek32(ps1.cpu.pc);
    write!(
        writer,
        "step={step} pc=0x{pc:08x} next=0x{next:08x} op=0x{opcode:08x}",
        pc = ps1.cpu.pc,
        next = ps1.cpu.next_pc,
    )
    .map_err(trace_write_error)?;
    for (index, value) in ps1.cpu.regs.iter().enumerate() {
        write!(writer, " r{index:02}=0x{value:08x}").map_err(trace_write_error)?;
    }
    writeln!(
        writer,
        " hi=0x{hi:08x} lo=0x{lo:08x} sr=0x{sr:08x} cause=0x{cause:08x} epc=0x{epc:08x} badv=0x{badv:08x} istat=0x{istat:04x} imask=0x{imask:04x}",
        hi = ps1.cpu.hi,
        lo = ps1.cpu.lo,
        sr = ps1.cpu.cop0.status(),
        cause = ps1.cpu.cop0.cause(),
        epc = ps1.cpu.cop0.epc(),
        badv = ps1.cpu.cop0.bad_vaddr(),
        istat = ps1.bus.irq.status(),
        imask = ps1.bus.irq.mask(),
    )
    .map_err(trace_write_error)
}

fn parse_addr_value(value: &str, name: &str) -> Result<(u32, u32), String> {
    let (left, right) = value
        .split_once('=')
        .ok_or_else(|| format!("{name} must use ADDR=VALUE"))?;
    Ok((parse_u32(left)?, parse_u32(right)?))
}

fn parse_reg_value(value: &str, name: &str) -> Result<(u8, u32), String> {
    let (left, right) = value
        .split_once('=')
        .ok_or_else(|| format!("{name} must use REG=VALUE"))?;
    Ok((parse_reg(left)?, parse_u32(right)?))
}

fn parse_disc_sector_size(value: &str) -> Result<CdromSectorSize, String> {
    match value.trim() {
        "2048" => Ok(CdromSectorSize::Cooked2048),
        "2352" => Ok(CdromSectorSize::Raw2352),
        _ => Err(format!("invalid --disc-sector-size value: {value}")),
    }
}

fn load_disc_image(
    path: &Path,
    forced_sector_size: Option<CdromSectorSize>,
) -> Result<(Vec<u8>, CdromSectorSize), String> {
    if is_cue_path(path) {
        let cue = fs::read_to_string(path)
            .map_err(|err| format!("failed to read CUE {}: {err}", path.display()))?;
        let spec = parse_cue_disc_spec(path, &cue)?;
        let mut image = fs::read(&spec.image_path)
            .map_err(|err| format!("failed to read disc {}: {err}", spec.image_path.display()))?;
        let byte_offset = spec
            .index01_frames
            .checked_mul(spec.sector_size.bytes())
            .ok_or_else(|| format!("CUE index offset overflows usize: {}", path.display()))?;
        if byte_offset > image.len() {
            return Err(format!(
                "CUE index offset exceeds image length: {}",
                path.display()
            ));
        }
        if byte_offset != 0 {
            image.drain(0..byte_offset);
        }
        let sector_size = forced_sector_size.unwrap_or(spec.sector_size);
        return Ok((image, sector_size));
    }

    let image =
        fs::read(path).map_err(|err| format!("failed to read disc {}: {err}", path.display()))?;
    let sector_size =
        forced_sector_size.unwrap_or_else(|| detect_disc_sector_size(path, image.len()));
    Ok((image, sector_size))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CueDiscSpec {
    image_path: PathBuf,
    sector_size: CdromSectorSize,
    index01_frames: usize,
}

fn parse_cue_disc_spec(cue_path: &Path, text: &str) -> Result<CueDiscSpec, String> {
    let cue_dir = cue_path.parent().unwrap_or_else(|| Path::new(""));
    let mut current_file: Option<PathBuf> = None;
    let mut selected: Option<CueDiscSpec> = None;
    let mut selected_track_open = false;
    let mut selected_has_index01 = false;

    for line in text.lines().map(str::trim) {
        if cue_line_is_ignored(line) {
            continue;
        }
        if let Some(file_path) = parse_cue_file_path(line) {
            let path = PathBuf::from(file_path);
            current_file = Some(if path.is_absolute() {
                path
            } else {
                cue_dir.join(path)
            });
            selected_track_open = false;
            continue;
        }
        if let Some(mode) = parse_cue_track_mode(line) {
            if selected.is_some() || mode.eq_ignore_ascii_case("AUDIO") {
                selected_track_open = false;
                continue;
            }
            let sector_size = cue_track_sector_size(mode)
                .ok_or_else(|| format!("unsupported CUE track mode: {mode}"))?;
            let image_path = current_file
                .clone()
                .ok_or_else(|| "CUE TRACK appears before FILE".to_string())?;
            selected = Some(CueDiscSpec {
                image_path,
                sector_size,
                index01_frames: 0,
            });
            selected_track_open = true;
            selected_has_index01 = false;
            continue;
        }
        if selected_track_open {
            if let Some(index01_frames) = parse_cue_index01(line)? {
                if let Some(spec) = selected.as_mut() {
                    spec.index01_frames = index01_frames;
                }
                selected_has_index01 = true;
                selected_track_open = false;
            }
        }
    }

    let selected =
        selected.ok_or_else(|| "CUE does not contain a supported data track".to_string())?;
    if !selected_has_index01 {
        return Err("CUE data track is missing INDEX 01".to_string());
    }
    Ok(selected)
}

fn parse_cue_file_path(line: &str) -> Option<String> {
    let rest = cue_keyword_rest(line, "FILE")?;
    if let Some(quoted) = rest.strip_prefix('"') {
        let end = quoted.find('"')?;
        Some(quoted[..end].to_string())
    } else {
        rest.split_whitespace().next().map(str::to_string)
    }
}

fn parse_cue_track_mode(line: &str) -> Option<&str> {
    if !cue_keyword_is(line, "TRACK") {
        return None;
    }
    let mut parts = line.split_whitespace();
    let _keyword = parts.next()?;
    let track_or_mode = parts.next()?;
    if track_or_mode.parse::<u8>().is_ok() {
        parts.next()
    } else {
        Some(track_or_mode)
    }
}

fn parse_cue_index01(line: &str) -> Result<Option<usize>, String> {
    if !cue_keyword_is(line, "INDEX") {
        return Ok(None);
    }
    let mut parts = line.split_whitespace();
    let _keyword = parts.next();
    let Some(index) = parts.next() else {
        return Err("malformed CUE INDEX line".to_string());
    };
    if index != "01" {
        return Ok(None);
    }
    let msf = parts
        .next()
        .ok_or_else(|| "CUE INDEX 01 requires MM:SS:FF".to_string())?;
    parse_cue_msf(msf).map(Some)
}

fn parse_cue_msf(value: &str) -> Result<usize, String> {
    let mut parts = value.split(':');
    let minute = parts
        .next()
        .ok_or_else(|| format!("invalid CUE MSF value: {value}"))?
        .parse::<usize>()
        .map_err(|_| format!("invalid CUE MSF value: {value}"))?;
    let second = parts
        .next()
        .ok_or_else(|| format!("invalid CUE MSF value: {value}"))?
        .parse::<usize>()
        .map_err(|_| format!("invalid CUE MSF value: {value}"))?;
    let frame = parts
        .next()
        .ok_or_else(|| format!("invalid CUE MSF value: {value}"))?
        .parse::<usize>()
        .map_err(|_| format!("invalid CUE MSF value: {value}"))?;
    if parts.next().is_some() || second >= 60 || frame >= 75 {
        return Err(format!("invalid CUE MSF value: {value}"));
    }
    Ok(((minute * 60) + second) * 75 + frame)
}

fn cue_track_sector_size(mode: &str) -> Option<CdromSectorSize> {
    match mode.to_ascii_uppercase().as_str() {
        "MODE1/2048" | "MODE2/2048" => Some(CdromSectorSize::Cooked2048),
        "MODE1/2352" | "MODE2/2352" | "MODE1_RAW" | "MODE2_RAW" | "CDI/2352" => {
            Some(CdromSectorSize::Raw2352)
        }
        _ => None,
    }
}

fn cue_line_is_ignored(line: &str) -> bool {
    line.is_empty()
        || line.starts_with(';')
        || line.starts_with("//")
        || cue_keyword_is(line, "REM")
        || cue_keyword_is(line, "CD_ROM")
        || cue_keyword_is(line, "CD_ROM_XA")
}

fn cue_keyword_is(line: &str, keyword: &str) -> bool {
    line.split_whitespace()
        .next()
        .is_some_and(|value| value.eq_ignore_ascii_case(keyword))
}

fn cue_keyword_rest<'a>(line: &'a str, keyword: &str) -> Option<&'a str> {
    let trimmed = line.trim_start();
    let keyword_len = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
    if trimmed[..keyword_len].eq_ignore_ascii_case(keyword) {
        Some(trimmed[keyword_len..].trim_start())
    } else {
        None
    }
}

fn is_cue_path(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("cue"))
}

fn detect_disc_sector_size(path: &Path, len: usize) -> CdromSectorSize {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let raw_sized = len % 2352 == 0;
    let raw_extension = matches!(extension.as_str(), "bin" | "img");
    if raw_sized && (raw_extension || len % 2048 != 0) {
        CdromSectorSize::Raw2352
    } else {
        CdromSectorSize::Cooked2048
    }
}

fn parse_reg(value: &str) -> Result<u8, String> {
    let normalized = value.trim().trim_start_matches('$');
    let reg = match normalized {
        "zero" => 0,
        "at" => 1,
        "v0" => 2,
        "v1" => 3,
        "a0" => 4,
        "a1" => 5,
        "a2" => 6,
        "a3" => 7,
        "t0" => 8,
        "t1" => 9,
        "t2" => 10,
        "t3" => 11,
        "t4" => 12,
        "t5" => 13,
        "t6" => 14,
        "t7" => 15,
        "s0" => 16,
        "s1" => 17,
        "s2" => 18,
        "s3" => 19,
        "s4" => 20,
        "s5" => 21,
        "s6" => 22,
        "s7" => 23,
        "t8" => 24,
        "t9" => 25,
        "k0" => 26,
        "k1" => 27,
        "gp" => 28,
        "sp" => 29,
        "fp" | "s8" => 30,
        "ra" => 31,
        _ => {
            let stripped = normalized.strip_prefix('r').unwrap_or(normalized);
            stripped
                .parse::<u8>()
                .map_err(|_| format!("invalid register: {value}"))?
        }
    };
    if reg <= 31 {
        Ok(reg)
    } else {
        Err(format!("register out of range: {value}"))
    }
}

fn parse_u32(value: &str) -> Result<u32, String> {
    let value = value.trim();
    let parsed = if let Some(hex) = value.strip_prefix("0x") {
        u32::from_str_radix(hex, 16)
    } else {
        value.parse()
    };
    parsed.map_err(|_| format!("invalid u32 value: {value}"))
}

fn trace_write_error(err: std::io::Error) -> String {
    format!("failed to write trace: {err}")
}

fn usage_and_exit() -> ! {
    eprintln!(
        "usage: ps1-frontend [--bios PATH] [--disc PATH] [--disc-sector-size 2048|2352] [--exe PATH] [--steps N] [--trace PATH] [--test] [--test-mailbox ADDR=PASS] [--test-stop-pc ADDR] [--test-pass-reg REG=VALUE] [--test-exit-reg REG]"
    );
    std::process::exit(2);
}

#[cfg(test)]
mod tests {
    use super::{
        detect_disc_sector_size, load_disc_image, parse_addr_value, parse_cue_disc_spec,
        parse_cue_msf, parse_disc_sector_size, parse_reg, parse_reg_value, parse_u32,
    };
    use ps1_core::cdrom::CdromSectorSize;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn parses_test_runner_cli_values() {
        assert_eq!(parse_u32("0x80010100").unwrap(), 0x8001_0100);
        assert_eq!(parse_u32("123").unwrap(), 123);
        assert_eq!(
            parse_addr_value("0x80010100=1", "--test-mailbox").unwrap(),
            (0x8001_0100, 1)
        );
        assert_eq!(parse_reg("v0").unwrap(), 2);
        assert_eq!(parse_reg("$a0").unwrap(), 4);
        assert_eq!(parse_reg("r31").unwrap(), 31);
        assert_eq!(
            parse_reg_value("v0=0x1234", "--test-pass-reg").unwrap(),
            (2, 0x1234)
        );
    }

    #[test]
    fn parses_and_detects_disc_sector_size() {
        assert_eq!(
            parse_disc_sector_size("2048").unwrap(),
            CdromSectorSize::Cooked2048
        );
        assert_eq!(
            parse_disc_sector_size("2352").unwrap(),
            CdromSectorSize::Raw2352
        );
        assert_eq!(
            detect_disc_sector_size(Path::new("game.iso"), 2048 * 16),
            CdromSectorSize::Cooked2048
        );
        assert_eq!(
            detect_disc_sector_size(Path::new("game.bin"), 2352 * 16),
            CdromSectorSize::Raw2352
        );
        assert_eq!(
            detect_disc_sector_size(Path::new("raw-dump.iso"), 2352 * 16),
            CdromSectorSize::Raw2352
        );
    }

    #[test]
    fn parses_cue_data_track_with_quoted_file_and_index() {
        let spec = parse_cue_disc_spec(
            Path::new("/games/sample/game.cue"),
            r#"
                REM Generated by a dump tool
                FILE "Game Disc.bin" BINARY
                  TRACK 01 MODE2/2352
                    INDEX 00 00:00:00
                    INDEX 01 00:02:00
            "#,
        )
        .unwrap();

        assert_eq!(
            spec.image_path,
            PathBuf::from("/games/sample/Game Disc.bin")
        );
        assert_eq!(spec.sector_size, CdromSectorSize::Raw2352);
        assert_eq!(spec.index01_frames, 150);
    }

    #[test]
    fn parses_cue_skipping_audio_track_before_data_track() {
        let spec = parse_cue_disc_spec(
            Path::new("disc.cue"),
            r#"
                FILE audio.bin BINARY
                  TRACK 01 AUDIO
                    INDEX 01 00:00:00
                FILE data.iso BINARY
                  TRACK 02 MODE1/2048
                    INDEX 01 00:00:20
            "#,
        )
        .unwrap();

        assert_eq!(spec.image_path, PathBuf::from("data.iso"));
        assert_eq!(spec.sector_size, CdromSectorSize::Cooked2048);
        assert_eq!(spec.index01_frames, 20);
    }

    #[test]
    fn parses_cue_comments_whitespace_and_raw_mode_alias() {
        let spec = parse_cue_disc_spec(
            Path::new("disc.cue"),
            r#"
                REM primary comment
                ; secondary comment
                // cdrdao-style comment
                CD_ROM_XA
                   FILE    "game.bin"    BINARY

                  TRACK MODE1_RAW
                    INDEX    01    00:00:00
            "#,
        )
        .unwrap();

        assert_eq!(spec.image_path, PathBuf::from("game.bin"));
        assert_eq!(spec.sector_size, CdromSectorSize::Raw2352);
        assert_eq!(spec.index01_frames, 0);
    }

    #[test]
    fn rejects_cue_data_track_without_index01() {
        let err = parse_cue_disc_spec(
            Path::new("disc.cue"),
            r#"
                FILE "game.bin" BINARY
                  TRACK 01 MODE2/2352
                    INDEX 00 00:00:00
            "#,
        )
        .unwrap_err();

        assert!(err.contains("INDEX 01"));
    }

    #[test]
    fn rejects_cue_track_before_file() {
        let err = parse_cue_disc_spec(
            Path::new("disc.cue"),
            r#"
                TRACK 01 MODE1/2048
                  INDEX 01 00:00:00
            "#,
        )
        .unwrap_err();

        assert!(err.contains("before FILE"));
    }

    #[test]
    fn load_disc_image_mounts_cue_and_applies_index_offset() {
        let dir = TempDir::new("ps1-cue-load");
        let bin = dir.path().join("disc.bin");
        let cue = dir.path().join("disc.cue");
        let mut image = vec![0xaa; 2048];
        image.extend(vec![0x11; 2048]);
        image.extend(vec![0x22; 2048]);
        fs::write(&bin, image).unwrap();
        fs::write(
            &cue,
            "FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:01\n",
        )
        .unwrap();

        let (loaded, sector_size) = load_disc_image(&cue, None).unwrap();

        assert_eq!(sector_size, CdromSectorSize::Cooked2048);
        assert_eq!(loaded.len(), 2048 * 2);
        assert_eq!(loaded[0], 0x11);
        assert_eq!(loaded[2048], 0x22);
    }

    #[test]
    fn load_disc_image_rejects_cue_index_past_image() {
        let dir = TempDir::new("ps1-cue-short");
        let bin = dir.path().join("disc.bin");
        let cue = dir.path().join("disc.cue");
        fs::write(&bin, vec![0; 2048]).unwrap();
        fs::write(
            &cue,
            "FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:02\n",
        )
        .unwrap();

        let err = load_disc_image(&cue, None).unwrap_err();

        assert!(err.contains("offset exceeds image length"));
    }

    #[test]
    fn parses_cue_msf_to_frames() {
        assert_eq!(parse_cue_msf("00:00:00").unwrap(), 0);
        assert_eq!(parse_cue_msf("01:02:03").unwrap(), 4653);
        assert!(parse_cue_msf("00:00:75").is_err());
    }

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(prefix: &str) -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path =
                std::env::temp_dir().join(format!("{prefix}-{}-{unique}", std::process::id()));
            fs::create_dir(&path).unwrap();
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
