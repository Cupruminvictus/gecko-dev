/* -*- Mode: C++; tab-width: 8; indent-tabs-mode: nil; c-basic-offset: 2 -*- */
/* vim: set ts=8 sts=2 et sw=2 tw=80: */
/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#include "LayersLogging.h"
#include <stdint.h>              // for uint8_t
#include "FrameMetrics.h"        // for FrameMetrics, etc
#include "ImageTypes.h"          // for ImageFormat
#include "mozilla/gfx/Matrix.h"  // for Matrix4x4, Matrix
#include "mozilla/gfx/Point.h"   // for IntSize
#include "nsDebug.h"             // for NS_ERROR
#include "nsPoint.h"             // for nsPoint
#include "nsRect.h"              // for nsRect
#include "nsRectAbsolute.h"      // for nsRectAbsolute
#include "base/basictypes.h"

using namespace mozilla::gfx;

namespace mozilla {
namespace layers {

void AppendToString(std::stringstream& aStream, TextureFlags flags,
                    const char* pfx, const char* sfx) {
  aStream << pfx;
  if (flags == TextureFlags::NO_FLAGS) {
    aStream << "NoFlags";
  } else {
#define AppendFlag(test)    \
  {                         \
    if (!!(flags & test)) { \
      if (previous) {       \
        aStream << "|";     \
      }                     \
      aStream << #test;     \
      previous = true;      \
    }                       \
  }
    bool previous = false;
    AppendFlag(TextureFlags::USE_NEAREST_FILTER);
    AppendFlag(TextureFlags::ORIGIN_BOTTOM_LEFT);
    AppendFlag(TextureFlags::DISALLOW_BIGIMAGE);

#undef AppendFlag
  }
  aStream << sfx;
}

void AppendToString(std::stringstream& aStream,
                    mozilla::gfx::SurfaceFormat format, const char* pfx,
                    const char* sfx) {
  aStream << pfx;
  switch (format) {
    case SurfaceFormat::B8G8R8A8:
      aStream << "SurfaceFormat::B8G8R8A8";
      break;
    case SurfaceFormat::B8G8R8X8:
      aStream << "SurfaceFormat::B8G8R8X8";
      break;
    case SurfaceFormat::R8G8B8A8:
      aStream << "SurfaceFormat::R8G8B8A8";
      break;
    case SurfaceFormat::R8G8B8X8:
      aStream << "SurfaceFormat::R8G8B8X8";
      break;
    case SurfaceFormat::R5G6B5_UINT16:
      aStream << "SurfaceFormat::R5G6B5_UINT16";
      break;
    case SurfaceFormat::A8:
      aStream << "SurfaceFormat::A8";
      break;
    case SurfaceFormat::YUV:
      aStream << "SurfaceFormat::YUV";
      break;
    case SurfaceFormat::NV12:
      aStream << "SurfaceFormat::NV12";
      break;
    case SurfaceFormat::P010:
      aStream << "SurfaceFormat::P010";
      break;
    case SurfaceFormat::P016:
      aStream << "SurfaceFormat::P016";
      break;
    case SurfaceFormat::YUV422:
      aStream << "SurfaceFormat::YUV422";
      break;
    case SurfaceFormat::UNKNOWN:
      aStream << "SurfaceFormat::UNKNOWN";
      break;
    default:
      NS_ERROR("unknown surface format");
      aStream << "???";
  }

  aStream << sfx;
}

void AppendToString(std::stringstream& aStream, gfx::SurfaceType aType,
                    const char* pfx, const char* sfx) {
  aStream << pfx;
  switch (aType) {
    case SurfaceType::DATA:
      aStream << "SurfaceType::DATA";
      break;
    case SurfaceType::D2D1_BITMAP:
      aStream << "SurfaceType::D2D1_BITMAP";
      break;
    case SurfaceType::D2D1_DRAWTARGET:
      aStream << "SurfaceType::D2D1_DRAWTARGET";
      break;
    case SurfaceType::CAIRO:
      aStream << "SurfaceType::CAIRO";
      break;
    case SurfaceType::CAIRO_IMAGE:
      aStream << "SurfaceType::CAIRO_IMAGE";
      break;
    case SurfaceType::COREGRAPHICS_IMAGE:
      aStream << "SurfaceType::COREGRAPHICS_IMAGE";
      break;
    case SurfaceType::COREGRAPHICS_CGCONTEXT:
      aStream << "SurfaceType::COREGRAPHICS_CGCONTEXT";
      break;
    case SurfaceType::SKIA:
      aStream << "SurfaceType::SKIA";
      break;
    case SurfaceType::DUAL_DT:
      aStream << "SurfaceType::DUAL_DT";
      break;
    case SurfaceType::D2D1_1_IMAGE:
      aStream << "SurfaceType::D2D1_1_IMAGE";
      break;
    case SurfaceType::RECORDING:
      aStream << "SurfaceType::RECORDING";
      break;
    case SurfaceType::WRAP_AND_RECORD:
      aStream << "SurfaceType::WRAP_AND_RECORD";
      break;
    case SurfaceType::TILED:
      aStream << "SurfaceType::TILED";
      break;
    case SurfaceType::DATA_SHARED:
      aStream << "SurfaceType::DATA_SHARED";
      break;
    case SurfaceType::DATA_RECYCLING_SHARED:
      aStream << "SurfaceType::DATA_RECYCLING_SHARED";
      break;
    case SurfaceType::DATA_ALIGNED:
      aStream << "SurfaceType::DATA_ALIGNED";
      break;
    default:
      NS_ERROR("unknown surface type");
      aStream << "???";
  }
  aStream << sfx;
}

void AppendToString(std::stringstream& aStream, ImageFormat format,
                    const char* pfx, const char* sfx) {
  aStream << pfx;
  switch (format) {
    case ImageFormat::PLANAR_YCBCR:
      aStream << "ImageFormat::PLANAR_YCBCR";
      break;
    case ImageFormat::SHARED_RGB:
      aStream << "ImageFormat::SHARED_RGB";
      break;
    case ImageFormat::CAIRO_SURFACE:
      aStream << "ImageFormat::CAIRO_SURFACE";
      break;
    case ImageFormat::MAC_IOSURFACE:
      aStream << "ImageFormat::MAC_IOSURFACE";
      break;
    case ImageFormat::SURFACE_TEXTURE:
      aStream << "ImageFormat::SURFACE_TEXTURE";
      break;
    case ImageFormat::D3D9_RGB32_TEXTURE:
      aStream << "ImageFormat::D3D9_RBG32_TEXTURE";
      break;
    case ImageFormat::OVERLAY_IMAGE:
      aStream << "ImageFormat::OVERLAY_IMAGE";
      break;
    case ImageFormat::D3D11_SHARE_HANDLE_TEXTURE:
      aStream << "ImageFormat::D3D11_SHARE_HANDLE_TEXTURE";
      break;
    default:
      NS_ERROR("unknown image format");
      aStream << "???";
  }

  aStream << sfx;
}

void AppendToString(std::stringstream& aStream,
                    const mozilla::ScrollPositionUpdate& aUpdate,
                    const char* pfx, const char* sfx) {
  aStream << pfx;
  aUpdate.AppendToString(aStream);
  aStream << sfx;
}

}  // namespace layers
}  // namespace mozilla

void print_stderr(std::stringstream& aStr) {
#if defined(ANDROID)
  // On Android logcat output is truncated to 1024 chars per line, and
  // we usually use std::stringstream to build up giant multi-line gobs
  // of output. So to avoid the truncation we find the newlines and
  // print the lines individually.
  std::string line;
  while (std::getline(aStr, line)) {
    printf_stderr("%s\n", line.c_str());
  }
#else
  printf_stderr("%s", aStr.str().c_str());
#endif
}

void fprint_stderr(FILE* aFile, std::stringstream& aStr) {
  if (aFile == stderr) {
    print_stderr(aStr);
  } else {
    fprintf_stderr(aFile, "%s", aStr.str().c_str());
  }
}
