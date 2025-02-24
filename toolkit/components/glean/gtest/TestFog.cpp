/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#include "gtest/gtest.h"

#include "mozilla/Preferences.h"
#include "nsString.h"

using mozilla::Preferences;

extern "C" {
// This function is called by the rust code in test.rs if a non-fatal test
// failure occurs.
void GTest_FOG_ExpectFailure(const char* aMessage) {
  EXPECT_STREQ(aMessage, "");
}

nsresult fog_init();
nsresult fog_submit_ping(const nsACString* aPingName);
}

// Initialize FOG exactly once.
// This needs to be the first test to run!
TEST(FOG, FogInitDoesntCrash)
{ ASSERT_EQ(NS_OK, fog_init()); }

extern "C" void Rust_MeasureInitializeTime();
TEST(FOG, TestMeasureInitializeTime)
{ Rust_MeasureInitializeTime(); }

TEST(FOG, BuiltinPingsRegistered)
{
  Preferences::SetInt("telemetry.fog.test.localhost_port", -1);
  nsAutoCString metricsPingName("metrics");
  nsAutoCString baselinePingName("baseline");
  nsAutoCString eventsPingName("events");
  ASSERT_EQ(NS_OK, fog_submit_ping(&metricsPingName));
  // This will probably change to NS_OK once "duration" is implemented.
  ASSERT_EQ(NS_ERROR_NO_CONTENT, fog_submit_ping(&baselinePingName));
  ASSERT_EQ(NS_ERROR_NO_CONTENT, fog_submit_ping(&eventsPingName));
}
