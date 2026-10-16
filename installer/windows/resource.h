// SPDX-License-Identifier: GPL-3.0-or-later
// Resource identifiers shared between redrob_query_setup.rc and redrob_query_setup.cpp.
#pragma once

// RCDATA
#define IDR_PAYLOAD          101  // the release's windows zip (absent from the uninstaller)
#define IDR_BACKGROUND_PNG   102  // res/background.png
#define IDR_LOCKUP_PNG       103  // res/lockup.png -- the Redrob Query lockup, white, for the deep ground
#define IDR_FONT_SANS        104  // Pretendard SemiBold, when the design system delivery was present
#define IDR_UNINSTALLER      106  // the payload-less build of this app, written into the install folder

#define IDI_APPICON          105  // assets/app-icon/redrob-query.ico

#define IDR_MANIFEST           1  // per-monitor-v2 DPI + common controls
