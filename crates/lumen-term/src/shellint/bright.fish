# Bright shell integration for fish.
#
# Bright puts the directory of this file's `share` on XDG_DATA_DIRS, so fish
# reads it from vendor_conf.d at start, before the user's config.fish. Nothing
# of the user's is changed. fish 4 and later write the prompt marks themselves,
# so only older fish gets them here: OSC 133 A, B, C and D with the exit status,
# and OSC 7 with the working directory.

status is-interactive; or exit
set -q _bright_marks; and exit
set -g _bright_marks 1

if test (string sub -l 1 -- $version) -ge 4 2>/dev/null
    exit
end

function _bright_prompt_mark --on-event fish_prompt
    printf '\e]7;file://%s%s\a\e]133;A\a' (hostname) (string escape --style=url -- $PWD | string replace -a '%2F' '/')
end

function _bright_preexec --on-event fish_preexec
    printf '\e]133;C\a'
end

function _bright_postexec --on-event fish_postexec
    printf '\e]133;D;%s\a' $status
end

# The end of the prompt: wrap the user's prompt function, whichever it is.
functions -q fish_prompt; and functions -c fish_prompt _bright_user_prompt
function fish_prompt
    _bright_user_prompt
    printf '\e]133;B\a'
end
