#include <setjmp.h>

#include "SignalHandlers.hpp"

extern "C"
int WasmtimeCallTrampoline(
    void **buf_storage,
    void *vmctx,
    void *caller_vmctx,
    void *stack_limit,
    void (*body)(void*, void*, void*, void*),
    void *args)
{
  jmp_buf buf;
  if (setjmp(buf) != 0) {
    return 0;
  }
  *buf_storage = &buf;
  body(vmctx, caller_vmctx, stack_limit, args);
  return 1;
}

extern "C"
int WasmtimeCall(
  void **buf_storage,
  void *vmctx,
  void *caller_vmctx,
  void *stack_limit,
  void (*body)(void*, void*, void*)) {
  jmp_buf buf;
  if (setjmp(buf) != 0) {
    return 0;
  }
  *buf_storage = &buf;
  body(vmctx, caller_vmctx, stack_limit);
  return 1;
}

extern "C"
void Unwind(void *JmpBuf) {
  jmp_buf *buf = (jmp_buf*) JmpBuf;
  longjmp(*buf, 1);
}
