/* -*- Mode: C++; tab-width: 8; indent-tabs-mode: nil; c-basic-offset: 2 -*- */
/* vim: set ts=8 sts=2 et sw=2 tw=80: */
/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#include "RenderTextureHostWrapper.h"

#include "mozilla/gfx/Logging.h"
#include "mozilla/webrender/RenderThread.h"

namespace mozilla {
namespace wr {

RenderTextureHostWrapper::RenderTextureHostWrapper(
    ExternalImageId aExternalImageId)
    : mExternalImageId(aExternalImageId) {
  MOZ_COUNT_CTOR_INHERITED(RenderTextureHostWrapper, RenderTextureHost);
}

RenderTextureHostWrapper::~RenderTextureHostWrapper() {
  MOZ_COUNT_DTOR_INHERITED(RenderTextureHostWrapper, RenderTextureHost);
}

void RenderTextureHostWrapper::EnsureTextureHost() const {
  if (!mTextureHost) {
    mTextureHost = RenderThread::Get()->GetRenderTexture(mExternalImageId);
    MOZ_ASSERT(mTextureHost);
    if (!mTextureHost) {
      gfxCriticalNoteOnce << "Failed to get RenderTextureHost for extId:"
                          << AsUint64(mExternalImageId);
    }
  }
}

wr::WrExternalImage RenderTextureHostWrapper::Lock(
    uint8_t aChannelIndex, gl::GLContext* aGL, wr::ImageRendering aRendering) {
  EnsureTextureHost();
  if (!mTextureHost) {
    return InvalidToWrExternalImage();
  }

  return mTextureHost->Lock(aChannelIndex, aGL, aRendering);
}

void RenderTextureHostWrapper::Unlock() {
  if (mTextureHost) {
    mTextureHost->Unlock();
  }
}

void RenderTextureHostWrapper::ClearCachedResources() {
  if (mTextureHost) {
    mTextureHost->ClearCachedResources();
  }
}

RenderMacIOSurfaceTextureHostOGL*
RenderTextureHostWrapper::AsRenderMacIOSurfaceTextureHostOGL() {
  EnsureTextureHost();
  if (!mTextureHost) {
    return nullptr;
  }
  return mTextureHost->AsRenderMacIOSurfaceTextureHostOGL();
}

RenderDXGITextureHostOGL*
RenderTextureHostWrapper::AsRenderDXGITextureHostOGL() {
  EnsureTextureHost();
  if (!mTextureHost) {
    return nullptr;
  }
  return mTextureHost->AsRenderDXGITextureHostOGL();
}

size_t RenderTextureHostWrapper::GetPlaneCount() {
  EnsureTextureHost();
  if (!mTextureHost) {
    return 0;
  }
  RenderTextureHostSWGL* swglHost = mTextureHost->AsRenderTextureHostSWGL();
  if (!swglHost) {
    return 0;
  }
  return swglHost->GetPlaneCount();
}

bool RenderTextureHostWrapper::MapPlane(uint8_t aChannelIndex,
                                        PlaneInfo& aPlaneInfo) {
  EnsureTextureHost();
  if (!mTextureHost) {
    return false;
  }
  RenderTextureHostSWGL* swglHost = mTextureHost->AsRenderTextureHostSWGL();
  if (!swglHost) {
    return false;
  }
  return swglHost->MapPlane(aChannelIndex, aPlaneInfo);
}

void RenderTextureHostWrapper::UnmapPlanes() {
  EnsureTextureHost();
  if (!mTextureHost) {
    return;
  }
  RenderTextureHostSWGL* swglHost = mTextureHost->AsRenderTextureHostSWGL();
  if (swglHost) {
    swglHost->UnmapPlanes();
  }
}

gfx::YUVColorSpace RenderTextureHostWrapper::GetYUVColorSpace() const {
  EnsureTextureHost();
  if (!mTextureHost) {
    return gfx::YUVColorSpace::UNKNOWN;
  }
  RenderTextureHostSWGL* swglHost = mTextureHost->AsRenderTextureHostSWGL();
  if (!swglHost) {
    return gfx::YUVColorSpace::UNKNOWN;
  }
  return swglHost->GetYUVColorSpace();
}

}  // namespace wr
}  // namespace mozilla
