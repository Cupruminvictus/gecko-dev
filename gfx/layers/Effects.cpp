/* -*- Mode: C++; tab-width: 8; indent-tabs-mode: nil; c-basic-offset: 2 -*- */
/* vim: set ts=8 sts=2 et sw=2 tw=80: */
/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#include "Effects.h"
#include "LayersLogging.h"  // for AppendToString
#include "nsAString.h"
#include "nsPrintfCString.h"  // for nsPrintfCString
#include "nsString.h"         // for nsAutoCString

using namespace mozilla::layers;

void TexturedEffect::PrintInfo(std::stringstream& aStream,
                               const char* aPrefix) {
  aStream << aPrefix;
  aStream << nsPrintfCString("%s (0x%p)", Name(), this).get()
          << " [texture-coords=" << mTextureCoords << "]";

  if (mPremultiplied) {
    aStream << " [premultiplied]";
  } else {
    aStream << " [not-premultiplied]";
  }

  aStream << " [filter=" << mSamplingFilter << "]";
}

void EffectMask::PrintInfo(std::stringstream& aStream, const char* aPrefix) {
  aStream << aPrefix << nsPrintfCString("EffectMask (0x%p)", this).get()
          << " [size=" << mSize << "]"
          << " [mask-transform=" << mMaskTransform << "]";
}

void EffectRenderTarget::PrintInfo(std::stringstream& aStream,
                                   const char* aPrefix) {
  TexturedEffect::PrintInfo(aStream, aPrefix);
  aStream << nsPrintfCString(" [render-target=%p]", mRenderTarget.get()).get();
}

void EffectSolidColor::PrintInfo(std::stringstream& aStream,
                                 const char* aPrefix) {
  aStream << aPrefix;
  aStream << nsPrintfCString("EffectSolidColor (0x%p) [color=%x]", this,
                             mColor.ToABGR())
                 .get();
}

void EffectBlendMode::PrintInfo(std::stringstream& aStream,
                                const char* aPrefix) {
  aStream << aPrefix;
  aStream << nsPrintfCString("EffectBlendMode (0x%p) [blendmode=%i]", this,
                             (int)mBlendMode)
                 .get();
}

void EffectColorMatrix::PrintInfo(std::stringstream& aStream,
                                  const char* aPrefix) {
  aStream << aPrefix;
  aStream << nsPrintfCString("EffectColorMatrix (0x%p)", this).get()
          << " [matrix=" << mColorMatrix << "]";
}
