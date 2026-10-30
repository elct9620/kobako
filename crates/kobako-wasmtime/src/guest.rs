//! One wasmtime instance as the runtime's host ABI endpoint drives it, from the
//! invocation body's `Store` and from a host import's `Caller` alike.

use kobako_runtime::endpoint::{Calls, GuestInstance};
use kobako_runtime::error::Trap;
use wasmtime::StoreContextMut;

use crate::invocation::Invocation;
use crate::trap;

pub(crate) struct Guest<'a>(pub(crate) StoreContextMut<'a, Invocation>);

impl GuestInstance for Guest<'_> {
    type Fault = wasmtime::Error;

    fn calls(&mut self) -> &mut Calls<wasmtime::Error> {
        self.0.data_mut().calls_mut()
    }

    fn memory(&mut self) -> Option<&mut [u8]> {
        let memory = self.0.data().exports()?.memory?;
        Some(memory.data_mut(&mut self.0))
    }

    fn alloc(&mut self, len: u32) -> Option<wasmtime::Result<u32>> {
        let alloc = self.0.data().exports()?.alloc?;
        Some(alloc.call(&mut self.0, len))
    }

    fn downcall(&mut self, ptr: i32, len: i32) -> Option<wasmtime::Result<u64>> {
        let downcall = self.0.data().exports()?.downcall?;
        Some(downcall.call(&mut self.0, (ptr, len)))
    }

    fn cap_trap(fault: &wasmtime::Error) -> Option<Trap> {
        trap::cap_trap(fault)
    }
}
