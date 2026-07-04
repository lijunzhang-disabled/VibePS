use crate::{audio, video};
use ps1_core::Ps1;
use std::env;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::PathBuf;

#[derive(Debug, Default)]
struct Args {
    bios: Option<PathBuf>,
    exe: Option<PathBuf>,
    trace: Option<PathBuf>,
    steps: u64,
}

pub fn run() -> Result<(), String> {
    let args = parse_args()?;

    let bios = args.bios.map(|path| {
        fs::read(&path).unwrap_or_else(|err| {
            eprintln!("failed to read BIOS {}: {err}", path.display());
            std::process::exit(1);
        })
    });

    let mut ps1 = Ps1::new(bios);
    if let Some(path) = args.exe {
        let exe = fs::read(&path)
            .map_err(|err| format!("failed to read EXE {}: {err}", path.display()))?;
        ps1.load_psx_exe(&exe)
            .map_err(|err| format!("failed to load EXE {}: {err:?}", path.display()))?;
    }

    let steps = args.steps.max(1);
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
            "--exe" => {
                args.exe = Some(PathBuf::from(iter.next().ok_or("--exe requires a path")?));
            }
            "--trace" => {
                args.trace = Some(PathBuf::from(iter.next().ok_or("--trace requires a path")?));
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

fn trace_write_error(err: std::io::Error) -> String {
    format!("failed to write trace: {err}")
}

fn usage_and_exit() -> ! {
    eprintln!("usage: ps1-frontend [--bios PATH] [--exe PATH] [--steps N] [--trace PATH]");
    std::process::exit(2);
}
