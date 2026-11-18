//! Token amounts as people write them and as contracts count them.
//!
//! A contract counts a token in base units: 1 USDC is `1000000`, since USDC
//! has 6 decimals. These convert between the two as decimal strings, so any
//! `uint256` amount works and the SDK needs no big-integer type. Carry
//! amounts as base units, and format them only where a person reads them.

/// The most decimals a token may have here: `10^77` is the largest power of
/// ten a `uint256` holds.
pub const MAX_DECIMALS: u32 = 77;

const UINT256_MAX: &str =
    "115792089237316195423570985008687907853269984665640564039457584007913129639935";

/// Convert an amount a person wrote, such as `"1.5"`, to base units, such as
/// `"1500000"` at 6 decimals.
///
/// Strict, because a misread amount moves the wrong sum: digits with at most
/// one `.`, and no sign, exponent, separator or padding. An amount finer
/// than the token can represent is refused, never rounded, though trailing
/// zeros are fine: `"1.50"` at 1 decimal is `"15"`. The result fits a
/// `uint256` and has no leading zeros.
pub fn parse_units(amount: &str, decimals: u32) -> Result<String, String> {
    check_decimals(decimals)?;
    let (whole, fraction) = amount.split_once('.').unwrap_or((amount, ""));
    let plain = [whole, fraction]
        .iter()
        .all(|part| part.bytes().all(|byte| byte.is_ascii_digit()));
    if !plain || (whole.is_empty() && fraction.is_empty()) {
        return Err(format!("amount {amount:?} is not a plain decimal number"));
    }
    let fraction = fraction.trim_end_matches('0');
    if fraction.len() > decimals as usize {
        return Err(format!(
            "amount {amount:?} has more than {decimals} decimal places"
        ));
    }
    let mut digits = String::with_capacity(whole.len() + decimals as usize);
    digits.push_str(whole);
    digits.push_str(fraction);
    digits.extend(std::iter::repeat_n('0', decimals as usize - fraction.len()));
    uint256(&digits)
        .map(str::to_owned)
        .ok_or_else(|| format!("amount {amount:?} is too large"))
}

/// Convert base units, such as `"1500000"`, to the amount a person reads,
/// such as `"1.5"` at 6 decimals. Trailing zeros are dropped, and a whole
/// amount has no `.`.
///
/// `base_units` is a decimal `uint256`, as a contract or an API returns it.
pub fn format_units(base_units: &str, decimals: u32) -> Result<String, String> {
    check_decimals(decimals)?;
    let digits = uint256(base_units)
        .ok_or_else(|| format!("{base_units:?} is not a uint256 in base units"))?;
    let decimals = decimals as usize;
    let padded = format!("{digits:0>width$}", width = decimals + 1);
    let (whole, fraction) = padded.split_at(padded.len() - decimals);
    let fraction = fraction.trim_end_matches('0');
    Ok(if fraction.is_empty() {
        whole.to_owned()
    } else {
        format!("{whole}.{fraction}")
    })
}

fn check_decimals(decimals: u32) -> Result<(), String> {
    if decimals > MAX_DECIMALS {
        return Err(format!(
            "{decimals} decimals is more than the {MAX_DECIMALS} a uint256 allows"
        ));
    }
    Ok(())
}

/// `digits` without leading zeros, if it is a decimal `uint256`.
fn uint256(digits: &str) -> Option<&str> {
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let trimmed = digits.trim_start_matches('0');
    let value = if trimmed.is_empty() { "0" } else { trimmed };
    // Equal-length digit strings order as the numbers do.
    let fits = value.len() < UINT256_MAX.len()
        || (value.len() == UINT256_MAX.len() && value <= UINT256_MAX);
    fits.then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_amount_becomes_base_units_at_the_token_s_decimals() {
        // USDC has 6 decimals and most vault shares 18; mixing them up
        // misprices an amount by twelve orders of magnitude.
        assert_eq!(parse_units("1", 6).unwrap(), "1000000");
        assert_eq!(parse_units("1", 18).unwrap(), "1000000000000000000");
        assert_eq!(parse_units("100.5", 6).unwrap(), "100500000");
        assert_eq!(parse_units("0.000001", 6).unwrap(), "1");
        assert_eq!(parse_units("7", 0).unwrap(), "7");
        assert_eq!(parse_units("0", 6).unwrap(), "0");
        assert_eq!(parse_units("0.0", 6).unwrap(), "0");
    }

    #[test]
    fn a_bare_point_on_either_side_reads_as_zero_there() {
        assert_eq!(parse_units(".5", 6).unwrap(), "500000");
        assert_eq!(parse_units("5.", 6).unwrap(), "5000000");
        assert_eq!(parse_units("007.5", 1).unwrap(), "75");
    }

    #[test]
    fn an_amount_finer_than_the_token_is_refused_not_rounded() {
        let error = parse_units("1.0000001", 6).unwrap_err();
        assert!(error.contains("more than 6 decimal places"), "{error}");
        assert!(parse_units("0.5", 0).is_err());
        // Trailing zeros lose nothing.
        assert_eq!(parse_units("1.50", 1).unwrap(), "15");
        assert_eq!(parse_units("2.000", 0).unwrap(), "2");
    }

    #[test]
    fn anything_but_a_plain_decimal_is_refused() {
        for amount in [
            "", ".", "-1", "+1", "1e6", "1,000", "1_000", " 1", "1 ", "1.2.3", "0x10", "١", "1.5\n",
        ] {
            assert!(parse_units(amount, 6).is_err(), "{amount:?}");
        }
    }

    #[test]
    fn the_result_always_fits_a_uint256() {
        assert_eq!(parse_units(UINT256_MAX, 0).unwrap(), UINT256_MAX);
        let over = "115792089237316195423570985008687907853269984665640564039457584007913129639936";
        assert!(parse_units(over, 0).unwrap_err().contains("too large"));
        // 10^77 fits; ten times it does not.
        assert_eq!(parse_units("1", 77).unwrap().len(), 78);
        assert!(parse_units("10", 77).is_err());
        assert!(parse_units("1", 78).unwrap_err().contains("78 decimals"));
        // A long run of leading zeros is still a small number.
        let padded = format!("{}1", "0".repeat(200));
        assert_eq!(parse_units(&padded, 0).unwrap(), "1");
    }

    #[test]
    fn base_units_read_back_without_trailing_zeros() {
        assert_eq!(format_units("100500000", 6).unwrap(), "100.5");
        assert_eq!(format_units("1000000", 6).unwrap(), "1");
        assert_eq!(format_units("0", 6).unwrap(), "0");
        assert_eq!(format_units("1", 6).unwrap(), "0.000001");
        assert_eq!(format_units("1005644", 6).unwrap(), "1.005644");
        assert_eq!(format_units("42", 0).unwrap(), "42");
        assert_eq!(format_units("000042", 1).unwrap(), "4.2");
        assert_eq!(
            format_units(UINT256_MAX, 18).unwrap(),
            "115792089237316195423570985008687907853269984665640564039457.584007913129639935"
        );
    }

    #[test]
    fn base_units_must_be_a_uint256() {
        for value in ["", "-1", "1.5", "1e3", " 1", "0x1"] {
            assert!(format_units(value, 6).is_err(), "{value:?}");
        }
        let over = format!("{UINT256_MAX}0");
        assert!(format_units(&over, 6).is_err());
        assert!(format_units("1", 78).is_err());
    }

    #[test]
    fn formatting_and_parsing_round_trip() {
        for (base, decimals) in [
            ("0", 0),
            ("1", 18),
            ("999999", 6),
            ("1000000", 6),
            ("123456789012345678901234567890", 18),
            (UINT256_MAX, 77),
            (UINT256_MAX, 0),
        ] {
            let shown = format_units(base, decimals).unwrap();
            assert_eq!(parse_units(&shown, decimals).unwrap(), base, "{shown}");
        }
    }
}
