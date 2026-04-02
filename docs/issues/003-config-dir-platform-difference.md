# 003: Config directory path differs by platform

## Summary

The `dirs::config_dir()` function returns platform-specific paths:
- **macOS**: `~/Library/Application Support/claudine/`
- **Linux**: `~/.config/claudine/`

The implementation spec and test commands reference `~/.config/claudine/` which is correct for Linux but not macOS. The code itself works correctly on both platforms since it uses `dirs::config_dir()` consistently.

## Impact

Documentation and test scripts that hardcode `~/.config/claudine/` paths will not find configs on macOS. The actual binary behavior is correct.

## Recommendation

No code change needed. Update documentation and test scripts to use platform-aware paths, or note the difference.
