use cranelift_codegen::binemit::Reloc;
use cranelift_codegen::isa::TargetIsa;
use instance::Instance;
use region::protect;
use region::Protection;
use std::mem::transmute;
use std::ptr::write_unaligned;
use wasmtime_environ::{compile_module, Compilation, Module, ModuleTranslation, Relocation};

/// Executes a module that has been translated with the `wasmtime-environ` environment
/// implementation.
pub fn compile_and_link_module<'data, 'module>(
    isa: &TargetIsa,
    translation: &ModuleTranslation<'data, 'module>,
) -> Result<Compilation, String> {
    debug_assert!(
        translation.module.start_func.is_none()
            || translation.module.start_func.unwrap() >= translation.module.imported_funcs.len(),
        "imported start functions not supported yet"
    );

    let (mut compilation, relocations) = compile_module(&translation, isa)?;

    // Apply relocations, now that we have virtual addresses for everything.
    relocate(&mut compilation, &relocations);

    Ok(compilation)
}

/// Performs the relocations inside the function bytecode, provided the necessary metadata
fn relocate(compilation: &mut Compilation, relocations: &[Vec<Relocation>]) {
    // The relocations are relative to the relocation's address plus four bytes
    // TODO: Support architectures other than x64, and other reloc kinds.
    for (i, function_relocs) in relocations.iter().enumerate() {
        for r in function_relocs {
            let target_func_address: isize = compilation.functions[r.func_index].as_ptr() as isize;
            let body = &mut compilation.functions[i];
            match r.reloc {
                Reloc::Abs8 => unsafe {
                    let reloc_address = body.as_mut_ptr().offset(r.offset as isize) as i64;
                    let reloc_addend = r.addend;
                    let reloc_abs = target_func_address as i64 + reloc_addend;
                    write_unaligned(reloc_address as *mut i64, reloc_abs);
                },
                Reloc::X86PCRel4 => unsafe {
                    let reloc_address = body.as_mut_ptr().offset(r.offset as isize) as isize;
                    let reloc_addend = r.addend as isize;
                    // TODO: Handle overflow.
                    let reloc_delta_i32 =
                        (target_func_address - reloc_address + reloc_addend) as i32;
                    write_unaligned(reloc_address as *mut i32, reloc_delta_i32);
                },
                _ => panic!("unsupported reloc kind"),
            }
        }
    }
}

/// Create the VmCtx data structure for the JIT'd code to use. This must
/// match the VmCtx layout in the environment.
fn make_vmctx(instance: &mut Instance) -> Vec<*mut u8> {
    let mut memories = Vec::new();
    let mut vmctx = Vec::new();
    vmctx.push(instance.globals.as_mut_ptr());
    for mem in &mut instance.memories {
        memories.push(mem.as_mut_ptr());
    }
    vmctx.push(memories.as_mut_ptr() as *mut u8);
    vmctx
}

/// Jumps to the code region of memory and execute the start function of the module.
pub fn execute(
    module: &Module,
    compilation: &Compilation,
    instance: &mut Instance,
) -> Result<(), String> {
    let start_index = module
        .start_func
        .ok_or_else(|| String::from("No start function defined, aborting execution"))?;
    // TODO: Put all the function bodies into a page-aligned memory region, and
    // then make them ReadExecute rather than ReadWriteExecute.
    for code_buf in &compilation.functions {
        match unsafe {
            protect(
                code_buf.as_ptr(),
                code_buf.len(),
                Protection::ReadWriteExecute,
            )
        } {
            Ok(()) => (),
            Err(err) => {
                return Err(format!(
                    "failed to give executable permission to code: {}",
                    err
                ))
            }
        }
    }

    let code_buf = &compilation.functions[start_index];

    let vmctx = make_vmctx(instance);

    // Rather than writing inline assembly to jump to the code region, we use the fact that
    // the Rust ABI for calling a function with no arguments and no return matches the one of
    // the generated code.Thanks to this, we can transmute the code region into a first-class
    // Rust function and call it.
    unsafe {
        let start_func = transmute::<_, fn(*const *mut u8)>(code_buf.as_ptr());
        start_func(vmctx.as_ptr());
    }
    Ok(())
}
