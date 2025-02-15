//! 32-bit ARM Instruction Set Architecture.

use crate::ir::condcodes::IntCC;
use crate::ir::Function;
use crate::isa::{Builder as IsaBuilder, TargetIsa};
use crate::machinst::{
    compile, MachCompileResult, MachTextSectionBuilder, TextSectionBuilder, VCode,
};
use crate::result::CodegenResult;
use crate::settings;

use alloc::{boxed::Box, vec::Vec};
use core::fmt;
use regalloc::{PrettyPrint, RealRegUniverse};
use target_lexicon::{Architecture, ArmArchitecture, Triple};

// New backend:
mod abi;
mod inst;
mod lower;
mod lower_inst;

use inst::{create_reg_universe, EmitInfo};

/// An ARM32 backend.
pub struct Arm32Backend {
    triple: Triple,
    flags: settings::Flags,
    reg_universe: RealRegUniverse,
}

impl Arm32Backend {
    /// Create a new ARM32 backend with the given (shared) flags.
    pub fn new_with_flags(triple: Triple, flags: settings::Flags) -> Arm32Backend {
        let reg_universe = create_reg_universe();
        Arm32Backend {
            triple,
            flags,
            reg_universe,
        }
    }

    fn compile_vcode(
        &self,
        func: &Function,
        flags: settings::Flags,
    ) -> CodegenResult<VCode<inst::Inst>> {
        // This performs lowering to VCode, register-allocates the code, computes
        // block layout and finalizes branches. The result is ready for binary emission.
        let emit_info = EmitInfo::new(flags.clone());
        let abi = Box::new(abi::Arm32ABICallee::new(func, flags)?);
        compile::compile::<Arm32Backend>(func, self, abi, &self.reg_universe, emit_info)
    }
}

impl TargetIsa for Arm32Backend {
    fn compile_function(
        &self,
        func: &Function,
        want_disasm: bool,
    ) -> CodegenResult<MachCompileResult> {
        let flags = self.flags();
        let vcode = self.compile_vcode(func, flags.clone())?;
        let (buffer, bb_starts, bb_edges) = vcode.emit();
        let frame_size = vcode.frame_size();
        let stackslot_offsets = vcode.stackslot_offsets().clone();

        let disasm = if want_disasm {
            Some(vcode.show_rru(Some(&create_reg_universe())))
        } else {
            None
        };

        let buffer = buffer.finish();

        Ok(MachCompileResult {
            buffer,
            frame_size,
            disasm,
            value_labels_ranges: Default::default(),
            stackslot_offsets,
            bb_starts,
            bb_edges,
        })
    }

    fn name(&self) -> &'static str {
        "arm32"
    }

    fn triple(&self) -> &Triple {
        &self.triple
    }

    fn flags(&self) -> &settings::Flags {
        &self.flags
    }

    fn isa_flags(&self) -> Vec<settings::Value> {
        Vec::new()
    }

    #[cfg(feature = "unwind")]
    fn emit_unwind_info(
        &self,
        _result: &MachCompileResult,
        _kind: crate::machinst::UnwindInfoKind,
    ) -> CodegenResult<Option<crate::isa::unwind::UnwindInfo>> {
        Ok(None) // FIXME implement this
    }

    fn unsigned_add_overflow_condition(&self) -> IntCC {
        // Carry flag set.
        IntCC::UnsignedGreaterThanOrEqual
    }

    fn text_section_builder(&self, num_funcs: u32) -> Box<dyn TextSectionBuilder> {
        Box::new(MachTextSectionBuilder::<inst::Inst>::new(num_funcs))
    }
}

impl fmt::Display for Arm32Backend {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("MachBackend")
            .field("name", &self.name())
            .field("triple", &self.triple())
            .field("flags", &format!("{}", self.flags()))
            .finish()
    }
}

/// Create a new `isa::Builder`.
pub fn isa_builder(triple: Triple) -> IsaBuilder {
    assert!(match triple.architecture {
        Architecture::Arm(ArmArchitecture::Arm)
        | Architecture::Arm(ArmArchitecture::Armv7)
        | Architecture::Arm(ArmArchitecture::Armv6) => true,
        _ => false,
    });
    IsaBuilder {
        triple,
        setup: settings::builder(),
        constructor: |triple, shared_flags, _| {
            let backend = Arm32Backend::new_with_flags(triple, shared_flags);
            Box::new(backend)
        },
    }
}
