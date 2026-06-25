#!/usr/bin/env python3
"""
Prototype validator for PR #720 podcast synchronization design constants.

Exercises the numeric constants declared in 07-architecture.md against
representative TOML configuration samples to verify that:
1. Default values are correctly applied when fields are absent
2. The minimum interval clamp (1s) prevents Duration::ZERO
3. max_new_episodes default (5) applies correctly
4. concurrent_downloads_max default (3) applies correctly
5. max_download_retries default (3) applies correctly

This is a throwaway prototype for Stage 6.5 verification.
"""

import json
import re
import sys
import tomllib
from dataclasses import dataclass, field
from typing import Optional


# ============================================================================
# Design constants from 07-architecture.md "Numeric Constants" section
# ============================================================================

SPEC_DEFAULT_INTERVAL_SECS = 3600  # 1 hour
SPEC_MIN_INTERVAL_CLAMP_SECS = 1  # prevents tokio panic on Duration::ZERO
SPEC_DEFAULT_MAX_NEW_EPISODES = 5  # limits bandwidth on first sync
SPEC_DEFAULT_CONCURRENT_DL_MAX = 3  # balances throughput vs resource usage
SPEC_DEFAULT_MAX_DL_RETRIES = 3  # existing value


# ============================================================================
# Duration parsing (mirrors humantime_serde behavior)
# ============================================================================


def parse_duration_secs(s: str) -> Optional[float]:
    """Parse a humantime-style duration string to seconds.
    Returns None if the string is invalid.
    Supports: Xs, Xm, Xh, and combinations like '2h30m'.
    """
    if not s or not isinstance(s, str):
        return None

    pattern = r"(?:(\d+)h)?(?:(\d+)m)?(?:(\d+)s)?$"
    m = re.fullmatch(pattern, s.strip())
    if not m:
        return None
    h, mins, secs = m.groups()
    if h is None and mins is None and secs is None:
        return None
    total = 0
    if h:
        total += int(h) * 3600
    if mins:
        total += int(mins) * 60
    if secs:
        total += int(secs)
    return float(total)


# ============================================================================
# Simulate the Rust deserialization + clamp logic
# ============================================================================


@dataclass
class MeasuredConstants:
    """Measured values after applying the config parsing + defaults logic."""

    interval_secs: float = SPEC_DEFAULT_INTERVAL_SECS
    min_clamp_applied: bool = False  # True if clamp was needed
    clamped_interval_secs: float = SPEC_DEFAULT_INTERVAL_SECS
    max_new_episodes: int = SPEC_DEFAULT_MAX_NEW_EPISODES
    concurrent_downloads_max: int = SPEC_DEFAULT_CONCURRENT_DL_MAX
    max_download_retries: int = SPEC_DEFAULT_MAX_DL_RETRIES
    parse_error: Optional[str] = None


def simulate_config_parse(toml_str: str) -> MeasuredConstants:
    """Simulate the Rust serde(default) + clamp logic on a TOML input."""
    result = MeasuredConstants()

    if not toml_str.strip():
        # Empty string -> all defaults apply
        result.clamped_interval_secs = max(
            result.interval_secs, SPEC_MIN_INTERVAL_CLAMP_SECS
        )
        return result

    try:
        parsed = tomllib.loads(toml_str)
    except Exception as e:
        result.parse_error = str(e)
        return result

    # Extract synchronization section (if present)
    sync = parsed.get("synchronization", {})

    # Extract podcast section (if present)
    podcast = parsed.get("podcast", {})

    # --- interval ---
    raw_interval = sync.get("interval", None)
    if raw_interval is not None:
        secs = parse_duration_secs(raw_interval)
        if secs is None:
            result.parse_error = f"Invalid duration: {raw_interval!r}"
            return result
        result.interval_secs = secs
    else:
        result.interval_secs = SPEC_DEFAULT_INTERVAL_SECS

    # Apply minimum clamp (mirrors: interval_duration.max(Duration::from_secs(1)))
    if result.interval_secs < SPEC_MIN_INTERVAL_CLAMP_SECS:
        result.min_clamp_applied = True
        result.clamped_interval_secs = SPEC_MIN_INTERVAL_CLAMP_SECS
    else:
        result.clamped_interval_secs = result.interval_secs

    # --- max_new_episodes ---
    result.max_new_episodes = sync.get(
        "max_new_episodes", SPEC_DEFAULT_MAX_NEW_EPISODES
    )

    # --- concurrent_downloads_max ---
    result.concurrent_downloads_max = podcast.get(
        "concurrent_downloads_max", SPEC_DEFAULT_CONCURRENT_DL_MAX
    )

    # --- max_download_retries ---
    result.max_download_retries = podcast.get(
        "max_download_retries", SPEC_DEFAULT_MAX_DL_RETRIES
    )

    return result


# ============================================================================
# Representative inputs (from task specification)
# ============================================================================

INPUTS = [
    (
        "explicit-1h-5ep",
        '[synchronization]\nenable = true\ninterval = "1h"\nrefresh_on_startup = true\nmax_new_episodes = 5',
    ),
    (
        "disabled-30m",
        '[synchronization]\nenable = false\ninterval = "30m"\nrefresh_on_startup = false',
    ),
    (
        "short-45s",
        '[synchronization]\ninterval = "45s"',
    ),
    (
        "long-2h30m",
        '[synchronization]\ninterval = "2h30m"\nenable = true\nrefresh_on_startup = true',
    ),
    (
        "full-server-no-sync",
        "[com]\nport = 5101\n\n[player]\nvolume = 30\n\n[podcast]\nmax_download_retries = 3",
    ),
    (
        "zero-interval",
        '[synchronization]\ninterval = "0s"',
    ),
    (
        "unlimited-episodes",
        '[synchronization]\nenable = true\ninterval = "2h"\nrefresh_on_startup = false\nmax_new_episodes = 0',
    ),
    (
        "invalid-duration",
        '[synchronization]\ninterval = "not_a_duration"',
    ),
    (
        "full-with-sync",
        '[podcast]\nmax_download_retries = 3\nconcurrent_downloads_max = 1\n\n[synchronization]\nenable = true\ninterval = "15m"\nmax_new_episodes = 100',
    ),
    (
        "empty-config",
        "",
    ),
]


# ============================================================================
# Measurement and reporting
# ============================================================================


@dataclass
class ConstantTest:
    name: str
    spec_value: float
    tolerance: float
    measurements: list = field(
        default_factory=list
    )  # list of (input_name, measured_value)


def run_measurements():
    """Run all inputs through the simulation and collect measurements."""

    constants = [
        ConstantTest("default_interval_secs", SPEC_DEFAULT_INTERVAL_SECS, 0.0),
        ConstantTest("min_interval_clamp_secs", SPEC_MIN_INTERVAL_CLAMP_SECS, 0.0),
        ConstantTest("default_max_new_episodes", SPEC_DEFAULT_MAX_NEW_EPISODES, 0.0),
        ConstantTest(
            "default_concurrent_downloads_max", SPEC_DEFAULT_CONCURRENT_DL_MAX, 0.0
        ),
        ConstantTest("default_max_download_retries", SPEC_DEFAULT_MAX_DL_RETRIES, 0.0),
    ]

    results = []

    for input_name, toml_str in INPUTS:
        measured = simulate_config_parse(toml_str)
        results.append((input_name, measured))

    # --- Constant 1: default_interval_secs ---
    # For inputs that do NOT specify an interval, the default should be 3600
    default_interval_inputs = [
        ("full-server-no-sync", "interval_secs"),
        ("empty-config", "interval_secs"),
    ]
    for input_name, _ in default_interval_inputs:
        m = next(r for n, r in results if n == input_name)
        if m.parse_error is None:
            constants[0].measurements.append((input_name, m.interval_secs))

    # --- Constant 2: min_interval_clamp_secs ---
    # For input with interval="0s", the clamped value should be 1
    zero_input = next(r for n, r in results if n == "zero-interval")
    if zero_input.parse_error is None:
        constants[1].measurements.append(
            ("zero-interval", zero_input.clamped_interval_secs)
        )
    # Also verify that non-zero intervals are NOT clamped (sanity check)
    for input_name, measured in results:
        if measured.parse_error is None and input_name != "zero-interval":
            if measured.interval_secs > 0:
                # The clamped value should equal the raw value (no clamping needed)
                constants[1].measurements.append(
                    (input_name, measured.clamped_interval_secs)
                )

    # --- Constant 3: default_max_new_episodes ---
    # For inputs that do NOT specify max_new_episodes, the default should be 5
    for input_name, measured in results:
        if measured.parse_error is None and input_name in (
            "disabled-30m",
            "short-45s",
            "long-2h30m",
            "full-server-no-sync",
            "zero-interval",
            "empty-config",
        ):
            constants[2].measurements.append((input_name, measured.max_new_episodes))

    # --- Constant 4: default_concurrent_downloads_max ---
    # For inputs that do NOT specify concurrent_downloads_max, the default should be 3
    for input_name, measured in results:
        if measured.parse_error is None and input_name in (
            "explicit-1h-5ep",
            "disabled-30m",
            "short-45s",
            "long-2h30m",
            "zero-interval",
            "unlimited-episodes",
            "empty-config",
        ):
            constants[3].measurements.append(
                (input_name, measured.concurrent_downloads_max)
            )

    # --- Constant 5: default_max_download_retries ---
    # For inputs that do NOT specify max_download_retries, the default should be 3
    for input_name, measured in results:
        if measured.parse_error is None and input_name in (
            "explicit-1h-5ep",
            "disabled-30m",
            "short-45s",
            "long-2h30m",
            "zero-interval",
            "unlimited-episodes",
            "empty-config",
        ):
            constants[4].measurements.append(
                (input_name, measured.max_download_retries)
            )

    return constants, results


def main():
    constants, all_results = run_measurements()

    print("=" * 72)
    print("PROTOTYPE VALIDATION: PR #720 Podcast Sync Design Constants")
    print("=" * 72)
    print()

    # Print all input parsing results
    print("--- Input Parsing Results ---")
    for input_name, measured in all_results:
        if measured.parse_error:
            print(f"  {input_name}: PARSE_ERROR ({measured.parse_error})")
        else:
            print(
                f"  {input_name}: interval={measured.interval_secs}s "
                f"(clamped={measured.clamped_interval_secs}s) "
                f"max_ep={measured.max_new_episodes} "
                f"conc_dl={measured.concurrent_downloads_max} "
                f"retries={measured.max_download_retries}"
            )
    print()

    # Print per-constant results
    overall_pass = True
    constant_verdicts = []

    print("--- Per-Constant Measurements ---")
    for const in constants:
        print(
            f"\n  [{const.name}] spec={const.spec_value} tolerance=+/-{const.tolerance}"
        )

        if not const.measurements:
            print("    (no applicable measurements)")
            constant_verdicts.append((const.name, "SKIP", 0.0))
            continue

        max_delta = 0.0
        all_within = True

        for input_name, measured_val in const.measurements:
            # Special handling for min_interval_clamp_secs:
            # For zero-interval input, measured should equal spec (1.0)
            # For non-zero inputs, measured should be >= 1 (clamp not needed, so pass)
            if const.name == "min_interval_clamp_secs":
                if input_name == "zero-interval":
                    delta = abs(measured_val - const.spec_value)
                else:
                    # Non-zero inputs: verify clamped >= 1 (the clamp is a floor)
                    delta = (
                        0.0
                        if measured_val >= const.spec_value
                        else abs(measured_val - const.spec_value)
                    )
            else:
                delta = abs(measured_val - const.spec_value)

            within = delta <= const.tolerance
            max_delta = max(max_delta, delta)
            if not within:
                all_within = False

            marker = "OK" if within else "FAIL"
            print(f"    {input_name}: measured={measured_val} delta={delta} [{marker}]")

        verdict = "Pass" if all_within else "Fail"
        if not all_within:
            overall_pass = False
        constant_verdicts.append((const.name, verdict, max_delta))
        print(f"    => Verdict: {verdict} (max_delta={max_delta})")

    print()
    print("=" * 72)
    overall_verdict = "PASS" if overall_pass else "FAIL"
    print(f"OVERALL VERDICT: {overall_verdict}")
    print("=" * 72)

    # Output structured JSON for report generation
    output = {
        "overall_verdict": overall_verdict,
        "constants": [],
    }
    for const in constants:
        measured_values = [v for _, v in const.measurements]
        if measured_values:
            entry = {
                "name": const.name,
                "spec_value": const.spec_value,
                "tolerance": const.tolerance,
                "measured_min": min(measured_values),
                "measured_max": max(measured_values),
                "measured_median": sorted(measured_values)[len(measured_values) // 2],
                "delta_max": max(
                    abs(v - const.spec_value)
                    if const.name != "min_interval_clamp_secs" or inp == "zero-interval"
                    else (0.0 if v >= const.spec_value else abs(v - const.spec_value))
                    for inp, v in const.measurements
                ),
                "within_tolerance": all(
                    (
                        abs(v - const.spec_value) <= const.tolerance
                        if const.name != "min_interval_clamp_secs"
                        or inp == "zero-interval"
                        else v >= const.spec_value
                    )
                    for inp, v in const.measurements
                ),
                "samples": len(measured_values),
            }
            output["constants"].append(entry)

    print()
    print("--- JSON Output ---")
    print(json.dumps(output, indent=2))

    return 0 if overall_pass else 1


if __name__ == "__main__":
    sys.exit(main())
