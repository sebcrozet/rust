// Copyright 2013 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use option::*;
use super::stack::StackSegment;
use libc::c_void;
use cast::{transmute, transmute_mut_unsafe,
           transmute_region, transmute_mut_region};

// FIXME #7761: Registers is boxed so that it is 16-byte aligned, for storing
// SSE regs.  It would be marginally better not to do this. In C++ we
// use an attribute on a struct.
// FIXME #7761: It would be nice to define regs as `~Option<Registers>` since
// the registers are sometimes empty, but the discriminant would
// then misalign the regs again.
pub struct Context {
    /// The context entry point, saved here for later destruction
    start: Option<~~fn()>,
    /// Hold the registers while the task or scheduler is suspended
    regs: ~Registers
}

impl Context {
    pub fn empty() -> Context {
        Context {
            start: None,
            regs: new_regs()
        }
    }

    /// Create a new context that will resume execution by running ~fn()
    pub fn new(start: ~fn(), stack: &mut StackSegment) -> Context {
        // FIXME #7767: Putting main into a ~ so it's a thin pointer and can
        // be passed to the spawn function.  Another unfortunate
        // allocation
        let start = ~start;

        // The C-ABI function that is the task entry point
        extern fn task_start_wrapper(f: &~fn()) { (*f)() }

        let fp: *c_void = task_start_wrapper as *c_void;
        let argp: *c_void = unsafe { transmute::<&~fn(), *c_void>(&*start) };
        let stack_base: *uint = stack.start();
        let sp: *uint = stack.end();
        let sp: *mut uint = unsafe { transmute_mut_unsafe(sp) };
        // Save and then immediately load the current context,
        // which we will then modify to call the given function when restored
        let mut regs = new_regs();
        unsafe {
            swap_registers(transmute_mut_region(&mut *regs), transmute_region(&*regs));
        };

        initialize_call_frame(&mut *regs, fp, argp, sp, stack_base);

        return Context {
            start: Some(start),
            regs: regs
        }
    }

    /* Switch contexts

    Suspend the current execution context and resume another by
    saving the registers values of the executing thread to a Context
    then loading the registers from a previously saved Context.
    */
    pub fn swap(out_context: &mut Context, in_context: &Context) {
        rtdebug!("swapping contexts");
        let out_regs: &mut Registers = match out_context {
            &Context { regs: ~ref mut r, _ } => r
        };
        let in_regs: &Registers = match in_context {
            &Context { regs: ~ref r, _ } => r
        };
        rtdebug!("doing raw swap");
        unsafe { swap_registers(out_regs, in_regs) };
    }
}

extern {
    #[rust_stack]
    fn swap_registers(out_regs: *mut Registers, in_regs: *Registers);
}

#[cfg(target_arch = "x86")]
struct Registers {
    eax: u32, ebx: u32, ecx: u32, edx: u32,
    ebp: u32, esi: u32, edi: u32, esp: u32,
    cs: u16, ds: u16, ss: u16, es: u16, fs: u16, gs: u16,
    eflags: u32, eip: u32
}

#[cfg(target_arch = "x86")]
fn new_regs() -> ~Registers {
    ~Registers {
        eax: 0, ebx: 0, ecx: 0, edx: 0,
        ebp: 0, esi: 0, edi: 0, esp: 0,
        cs: 0, ds: 0, ss: 0, es: 0, fs: 0, gs: 0,
        eflags: 0, eip: 0
    }
}

#[cfg(target_arch = "x86")]
fn initialize_call_frame(regs: &mut Registers, fptr: *c_void, arg: *c_void,
                         sp: *mut uint, _stack_base: *uint) {

    let sp = align_down(sp);
    let sp = mut_offset(sp, -4);

    unsafe { *sp = arg as uint };
    let sp = mut_offset(sp, -1);
    unsafe { *sp = 0 }; // The final return address

    regs.esp = sp as u32;
    regs.eip = fptr as u32;

    // Last base pointer on the stack is 0
    regs.ebp = 0;
}

#[cfg(windows, target_arch = "x86_64")]
type Registers = [uint, ..34];
#[cfg(not(windows), target_arch = "x86_64")]
type Registers = [uint, ..22];

#[cfg(windows, target_arch = "x86_64")]
fn new_regs() -> ~Registers { ~([0, .. 34]) }
#[cfg(not(windows), target_arch = "x86_64")]
fn new_regs() -> ~Registers { ~([0, .. 22]) }

#[cfg(target_arch = "x86_64")]
fn initialize_call_frame(regs: &mut Registers, fptr: *c_void, arg: *c_void,
                         sp: *mut uint, stack_base: *uint) {

    // Redefinitions from regs.h
    static RUSTRT_ARG0: uint = 3;
    static RUSTRT_RSP: uint = 1;
    static RUSTRT_IP: uint = 8;
    static RUSTRT_RBP: uint = 2;

    #[cfg(windows)]
    fn initialize_tib(regs: &mut Registers, sp: *mut uint, stack_base: *uint) {
        // Redefinitions from regs.h
        static RUSTRT_ST1: uint = 11; // stack bottom
        static RUSTRT_ST2: uint = 12; // stack top
        regs[RUSTRT_ST1] = sp as uint;
        regs[RUSTRT_ST2] = stack_base as uint;
    }
    #[cfg(not(windows))]
    fn initialize_tib(_: &mut Registers, _: *mut uint, _: *uint) {
    }

    // Win64 manages stack range at TIB: %gs:0x08 (top) and %gs:0x10 (bottom)
    initialize_tib(regs, sp, stack_base);

    let sp = align_down(sp);
    let sp = mut_offset(sp, -1);

    // The final return address. 0 indicates the bottom of the stack
    unsafe { *sp = 0; }

    rtdebug!("creating call frame");
    rtdebug!("fptr {}", fptr as uint);
    rtdebug!("arg {}", arg as uint);
    rtdebug!("sp {}", sp as uint);

    regs[RUSTRT_ARG0] = arg as uint;
    regs[RUSTRT_RSP] = sp as uint;
    regs[RUSTRT_IP] = fptr as uint;

    // Last base pointer on the stack should be 0
    regs[RUSTRT_RBP] = 0;
}

#[cfg(target_arch = "arm")]
type Registers = [uint, ..32];

#[cfg(target_arch = "arm")]
fn new_regs() -> ~Registers { ~([0, .. 32]) }

#[cfg(target_arch = "arm")]
fn initialize_call_frame(regs: &mut Registers, fptr: *c_void, arg: *c_void,
                         sp: *mut uint, _stack_base: *uint) {
    let sp = align_down(sp);
    // sp of arm eabi is 8-byte aligned
    let sp = mut_offset(sp, -2);

    // The final return address. 0 indicates the bottom of the stack
    unsafe { *sp = 0; }

    regs[0] = arg as uint;   // r0
    regs[13] = sp as uint;   // #53 sp, r13
    regs[14] = fptr as uint; // #60 pc, r15 --> lr
}

#[cfg(target_arch = "mips")]
type Registers = [uint, ..32];

#[cfg(target_arch = "mips")]
fn new_regs() -> ~Registers { ~([0, .. 32]) }

#[cfg(target_arch = "mips")]
fn initialize_call_frame(regs: &mut Registers, fptr: *c_void, arg: *c_void,
                         sp: *mut uint, _stack_base: *uint) {
    let sp = align_down(sp);
    // sp of mips o32 is 8-byte aligned
    let sp = mut_offset(sp, -2);

    // The final return address. 0 indicates the bottom of the stack
    unsafe { *sp = 0; }

    regs[4] = arg as uint;
    regs[29] = sp as uint;
    regs[25] = fptr as uint;
    regs[31] = fptr as uint;
}

fn align_down(sp: *mut uint) -> *mut uint {
    unsafe {
        let sp: uint = transmute(sp);
        let sp = sp & !(16 - 1);
        transmute::<uint, *mut uint>(sp)
    }
}

// ptr::mut_offset is positive ints only
#[inline]
pub fn mut_offset<T>(ptr: *mut T, count: int) -> *mut T {
    use std::sys::size_of;
    (ptr as int + count * (size_of::<T>() as int)) as *mut T
}
