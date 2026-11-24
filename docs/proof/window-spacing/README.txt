Real output of the terminal's headless --snapshot at scale 2, dark theme,
images off (so the chart block shows its alt text). Each scene is a fixed
byte stream played by /bin/sh with output processing off.

rich-dark-before.png  main before this change, 1200x1000
rich-dark-after.png   this branch, 1200x1000
grid-dark-after.png   this branch, 1200x760: the right and bottom insets
                      and the scroll bar in the right inset

The after captures are byte-identical across the branch's merges from main:
the merged changes do not alter these scenes.
