/* -*- Mode: C++; tab-width: 8; indent-tabs-mode: nil; c-basic-offset: 2 -*-
 * vim: set ts=8 sts=2 et sw=2 tw=80:
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#ifndef jit_ABIFunctions_h
#define jit_ABIFunctions_h

namespace js {
namespace jit {

// This class is used to ensure that all known targets of callWithABI are
// registered here. Otherwise, this would raise a static assertion at compile
// time.
template <typename Sig, Sig fun>
struct ABIFunctionData {
  static const bool registered = false;
};

template <typename Sig, Sig fun>
struct ABIFunction {
  void* address() const { return JS_FUNC_TO_DATA_PTR(void*, fun); }

  // If this assertion fails, you are likely in the context of a
  // `callWithABI<Sig, fn>()` call. This error indicates that ABIFunction has
  // not been specialized for `<Sig, fn>` by the time of this call.
  //
  // This can be fixed by adding the function signature to either
  // ABIFUNCTION_LIST or ABIFUNCTION_AND_TYPE_LIST (if overloaded) within
  // `ABIFunctionList-inl.h` and to add an `#include` statement of this header
  // in the file which is making the call to `callWithABI<Sig, fn>()`.
  static_assert(ABIFunctionData<Sig, fun>::registered,
                "ABI function is not registered.");
};

}  // namespace jit
}  // namespace js

#endif /* jit_VMFunctions_h */
