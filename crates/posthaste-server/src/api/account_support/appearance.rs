use super::*;

/// Normalize user-supplied appearance strings while preserving the selected mode.
#[cfg(test)]
pub(crate) fn normalize_account_appearance(appearance: AccountAppearance) -> AccountAppearance {
    match appearance {
        AccountAppearance::Initials {
            initials,
            color_hue,
        } => AccountAppearance::Initials {
            initials: normalize_initials(&initials),
            color_hue: color_hue.min(360),
        },
        AccountAppearance::Image {
            image_id,
            initials,
            color_hue,
        } => AccountAppearance::Image {
            image_id: image_id.trim().to_string(),
            initials: normalize_initials(&initials),
            color_hue: color_hue.min(360),
        },
    }
}

#[cfg(test)]
pub(crate) fn validate_account_appearance(appearance: &AccountAppearance) -> Result<(), ApiError> {
    let (initials, color_hue, image_id) = match appearance {
        AccountAppearance::Initials {
            initials,
            color_hue,
        } => (initials, color_hue, None),
        AccountAppearance::Image {
            image_id,
            initials,
            color_hue,
        } => (initials, color_hue, Some(image_id)),
    };
    if initials.trim().is_empty() || initials.chars().count() > 4 {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidAccount,
            "account appearance initials must be 1-4 characters",
        ));
    }
    if *color_hue > 360 {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidAccount,
            "account appearance color hue must be between 0 and 360",
        ));
    }
    if let Some(image_id) = image_id {
        validate_logo_image_id(image_id)?;
    }
    Ok(())
}

pub(crate) fn validate_logo_image_id(image_id: &str) -> Result<(), ApiError> {
    let is_valid = !image_id.is_empty()
        && image_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-');
    if !is_valid {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidAccountLogo,
            "account logo image id is invalid",
        ));
    }
    Ok(())
}

#[cfg(test)]
fn normalize_initials(value: &str) -> String {
    let words: Vec<&str> = value
        .split_whitespace()
        .filter(|word| !word.is_empty())
        .collect();
    let raw = if words.len() >= 2 {
        words
            .iter()
            .take(2)
            .filter_map(|word| word.chars().next())
            .collect::<String>()
    } else {
        value
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .take(2)
            .collect()
    };
    let normalized = raw.trim().to_uppercase();
    if normalized.is_empty() {
        "A".to_string()
    } else {
        normalized.chars().take(4).collect()
    }
}
