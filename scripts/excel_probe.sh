#!/bin/bash
# Feedback loop: open an .xlsx in Microsoft Excel and detect the
# "We found a problem with some content" repair prompt.
# Usage: excel_probe.sh <file.xlsx>
# Exit 0 (GREEN) = opened without repair prompt.
# Exit 1 (RED)   = repair prompt appeared.
set -u
FILE="$1"

osascript -e 'tell application "Microsoft Excel" to quit saving no' >/dev/null 2>&1
sleep 2
open -a "Microsoft Excel" "$FILE"

# Poll up to 25s for either the repair dialog (RED) or a workbook window (GREEN)
for i in $(seq 1 25); do
  sleep 1
  STATE=$(osascript <<'OSA' 2>/dev/null
tell application "System Events"
  tell process "Microsoft Excel"
    repeat with w in windows
      try
        set btns to name of every button of w
        if btns contains "Yes" and btns contains "No" then
          return "REPAIR"
        end if
      end try
    end repeat
    return "OPEN"
  end tell
end tell
OSA
)
  if [ "$STATE" = "REPAIR" ]; then
    echo "RED: repair prompt detected for $FILE"
    osascript -e 'tell application "System Events" to tell process "Microsoft Excel" to click button "No" of window 1' >/dev/null
    sleep 2
    osascript -e 'tell application "Microsoft Excel" to quit saving no' >/dev/null 2>&1
    exit 1
  fi
  if [ "$STATE" = "OPEN" ]; then
    # Give repair prompt extra chance to appear (Excel sometimes delays)
    sleep 4
    STATE2=$(osascript -e 'tell application "System Events" to tell process "Microsoft Excel"
      repeat with w in windows
        try
          set btns to name of every button of w
          if btns contains "Yes" and btns contains "No" then return "REPAIR"
        end try
      end repeat
      return "OPEN"
    end tell' 2>/dev/null)
    osascript -e 'tell application "Microsoft Excel" to quit saving no' >/dev/null 2>&1
    if [ "$STATE2" = "REPAIR" ]; then
      echo "RED (delayed): repair prompt detected for $FILE"
      exit 1
    fi
    echo "GREEN: $FILE opened without repair prompt"
    exit 0
  fi
done
echo "INCONCLUSIVE: Excel did not open workbook in time"
osascript -e 'tell application "Microsoft Excel" to quit saving no' >/dev/null 2>&1
exit 2
