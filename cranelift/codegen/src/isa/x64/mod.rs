//! X86_64-bit Instruction Set Architecture.

use self::inst::EmitInfo;

use super::TargetIsa;
use crate::ir::{condcodes::IntCC, Function};
#[cfg(feature = "unwind")]
use crate::isa::unwind::systemv;
use crate::isa::x64::{inst::regs::create_reg_universe_systemv, settings as x64_settings};
use crate::isa::Builder as IsaBuilder;
use crate::machinst::{
    compile, MachCompileResult, MachTextSectionBuilder, TextSectionBuilder, VCode,
};
use crate::result::CodegenResult;
use crate::settings::{self as shared_settings, Flags};
use alloc::{boxed::Box, vec::Vec};
use core::fmt;

use regalloc::{PrettyPrint, RealRegUniverse, Reg};
use target_lexicon::Triple;

mod abi;
pub mod encoding;
mod inst;
mod lower;
mod settings;

/// An X64 backend.
pub(crate) struct X64Backend {
    triple: Triple,
    flags: Flags,
    x64_flags: x64_settings::Flags,
    reg_universe: RealRegUniverse,
}

impl X64Backend {
    /// Create a new X64 backend with the given (shared) flags.
    fn new_with_flags(triple: Triple, flags: Flags, x64_flags: x64_settings::Flags) -> Self {
        let reg_universe = create_reg_universe_systemv(&flags);
        Self {
            triple,
            flags,
            x64_flags,
            reg_universe,
        }
    }

    fn compile_vcode(&self, func: &Function, flags: Flags) -> CodegenResult<VCode<inst::Inst>> {
        // This performs lowering to VCode, register-allocates the code, computes
        // block layout and finalizes branches. The result is ready for binary emission.
        let emit_info = EmitInfo::new(flags.clone(), self.x64_flags.clone());
        let abi = Box::new(abi::X64ABICallee::new(&func, flags)?);
        compile::compile::<Self>(&func, self, abi, &self.reg_universe, emit_info)
    }
}

impl TargetIsa for X64Backend {
    fn compile_function(
        &self,
        func: &Function,
        want_disasm: bool,
    ) -> CodegenResult<MachCompileResult> {
        let flags = self.flags();
        let vcode = self.compile_vcode(func, flags.clone())?;

        let (buffer, bb_starts, bb_edges) = vcode.emit();
        let buffer = buffer.finish();
        let frame_size = vcode.frame_size();
        let value_labels_ranges = vcode.value_labels_ranges();
        let stackslot_offsets = vcode.stackslot_offsets().clone();

        let disasm = if want_disasm {
            Some(vcode.show_rru(Some(&create_reg_universe_systemv(flags))))
        } else {
            None
        };

        Ok(MachCompileResult {
            buffer,
            frame_size,
            disasm,
            value_labels_ranges,
            stackslot_offsets,
            bb_starts,
            bb_edges,
        })
    }

    fn flags(&self) -> &Flags {
        &self.flags
    }

    fn isa_flags(&self) -> Vec<shared_settings::Value> {
        self.x64_flags.iter().collect()
    }

    fn name(&self) -> &'static str {
        "x64"
    }

    fn triple(&self) -> &Triple {
        &self.triple
    }

    fn unsigned_add_overflow_condition(&self) -> IntCC {
        // Unsigned `<`; this corresponds to the carry flag set on x86, which
        // indicates an add has overflowed.
        IntCC::UnsignedLessThan
    }

    #[cfg(feature = "unwind")]
    fn emit_unwind_info(
        &self,
        result: &MachCompileResult,
        kind: crate::machinst::UnwindInfoKind,
    ) -> CodegenResult<Option<crate::isa::unwind::UnwindInfo>> {
        use crate::isa::unwind::UnwindInfo;
        use crate::machinst::UnwindInfoKind;
        Ok(match kind {
            UnwindInfoKind::SystemV => {
                let mapper = self::inst::unwind::systemv::RegisterMapper;
                Some(UnwindInfo::SystemV(
                    crate::isa::unwind::systemv::create_unwind_info_from_insts(
                        &result.buffer.unwind_info[..],
                        result.buffer.data.len(),
                        &mapper,
                    )?,
                ))
            }
            UnwindInfoKind::Windows => Some(UnwindInfo::WindowsX64(
                crate::isa::unwind::winx64::create_unwind_info_from_insts::<
                    self::inst::unwind::winx64::RegisterMapper,
                >(&result.buffer.unwind_info[..])?,
            )),
            _ => None,
        })
    }

    #[cfg(feature = "unwind")]
    fn create_systemv_cie(&self) -> Option<gimli::write::CommonInformationEntry> {
        Some(inst::unwind::systemv::create_cie())
    }

    #[cfg(feature = "unwind")]
    fn map_regalloc_reg_to_dwarf(&self, reg: Reg) -> Result<u16, systemv::RegisterMappingError> {
        inst::unwind::systemv::map_reg(reg).map(|reg| reg.0)
    }

    fn text_section_builder(&self, num_funcs: u32) -> Box<dyn TextSectionBuilder> {
        Box::new(MachTextSectionBuilder::<inst::Inst>::new(num_funcs))
    }
}

impl fmt::Display for X64Backend {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("MachBackend")
            .field("name", &self.name())
            .field("triple", &self.triple())
            .field("flags", &format!("{}", self.flags()))
            .finish()
    }
}

/// Create a new `isa::Builder`.
pub(crate) fn isa_builder(triple: Triple) -> IsaBuilder {
    IsaBuilder {
        triple,
        setup: x64_settings::builder(),
        constructor: isa_constructor,
    }
}

fn isa_constructor(
    triple: Triple,
    shared_flags: Flags,
    builder: shared_settings::Builder,
) -> Box<dyn TargetIsa> {
    let isa_flags = x64_settings::Flags::new(&shared_flags, builder);
    let backend = X64Backend::new_with_flags(triple, shared_flags, isa_flags);
    Box::new(backend)
}
