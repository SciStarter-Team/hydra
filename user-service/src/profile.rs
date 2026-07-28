use anyhow::{Error, anyhow};
use async_nats::service::Request;
use common::{
    message::dashboard::{PublicProfile, PublicProfileVisibility, UserProfile, UserProfileUpdate},
    message::user::PRIVATE_DISPLAY_NAME,
    service::decode_structured_message,
    shared::db,
    ulid::AsUlid,
};

pub async fn get(req: &Request) -> Result<UserProfile, Error> {
    let user_id: String = flexbuffers::from_slice(&req.message.payload)?;

    let row = sqlx::query!(
        r#"SELECT
            "user_id",
            "handle",
            "first_name",
            "last_name",
            "primary_email",
            "image",
            "bio",
            "birthday",
            "gender",
            "zipcode",
            "country",
            "country_code",
            "mailing_address",
            "facebook_url",
            "twitter_profile",
            "home_page",
            "inat_username",
            "zooniverse_username",
            "orcid",
            "degree",
            "ethnicity",
            "income",
            "scientist" as "scientist: bool",
            "educator" as "educator: bool",
            "ambassador",
            "public" as "public: bool",
            "protected" as "protected: bool",
            "accept_emails" as "accept_emails: bool",
            "newsletter" as "newsletter: bool",
            "accept_messages" as "accept_messages: bool",
            "track_contributions" as "track_contributions: bool",
            "ai_opt_out" as "ai_opt_out: bool",
            "date_joined",
            AsText("point") as "point: String",
            "extra_skills",
            "extra_tools"
        FROM "users"
        WHERE "user_id" = ?"#,
        user_id
    )
    .fetch_optional(db())
    .await?
    .ok_or_else(|| anyhow!("User not found: {}", user_id))?;

    Ok(UserProfile {
        user_id: row.user_id.as_ulid()?,
        handle: row.handle,
        first_name: row.first_name,
        last_name: row.last_name,
        primary_email: row.primary_email,
        image: row.image,
        bio: row.bio,
        birthday: row.birthday,
        gender: row.gender,
        zipcode: row.zipcode,
        country: row.country,
        country_code: row.country_code,
        mailing_address: row.mailing_address,
        facebook_url: row.facebook_url,
        twitter_profile: row.twitter_profile,
        home_page: row.home_page,
        inat_username: row.inat_username,
        zooniverse_username: row.zooniverse_username,
        orcid: row.orcid,
        degree: row.degree,
        ethnicity: row.ethnicity,
        income: row.income,
        scientist: row.scientist,
        educator: row.educator,
        ambassador: row.ambassador,
        public: row.public,
        protected: row.protected,
        accept_emails: row.accept_emails,
        newsletter: row.newsletter,
        accept_messages: row.accept_messages,
        track_contributions: row.track_contributions,
        ai_opt_out: row.ai_opt_out,
        date_joined: row.date_joined,
        point: row.point,
        extra_skills: row.extra_skills,
        extra_tools: row.extra_tools,
    })
}

pub async fn save(req: &Request) -> Result<(), Error> {
    let update: UserProfileUpdate = decode_structured_message(req.message.clone())?;
    let user_id_str = update.user_id.to_string();

    sqlx::query!(
        r#"UPDATE "users" SET
            "handle" = COALESCE(?, "handle"),
            "first_name" = COALESCE(?, "first_name"),
            "last_name" = COALESCE(?, "last_name"),
            "image" = COALESCE(?, "image"),
            "bio" = COALESCE(?, "bio"),
            "birthday" = COALESCE(?, "birthday"),
            "gender" = COALESCE(?, "gender"),
            "zipcode" = COALESCE(?, "zipcode"),
            "country" = COALESCE(?, "country"),
            "country_code" = COALESCE(?, "country_code"),
            "mailing_address" = COALESCE(?, "mailing_address"),
            "facebook_url" = COALESCE(?, "facebook_url"),
            "twitter_profile" = COALESCE(?, "twitter_profile"),
            "home_page" = COALESCE(?, "home_page"),
            "inat_username" = COALESCE(?, "inat_username"),
            "zooniverse_username" = COALESCE(?, "zooniverse_username"),
            "orcid" = COALESCE(?, "orcid"),
            "degree" = COALESCE(?, "degree"),
            "ethnicity" = COALESCE(?, "ethnicity"),
            "income" = COALESCE(?, "income"),
            "scientist" = COALESCE(?, "scientist"),
            "educator" = COALESCE(?, "educator"),
            "public" = COALESCE(?, "public"),
            "protected" = COALESCE(?, "protected"),
            "accept_emails" = COALESCE(?, "accept_emails"),
            "newsletter" = COALESCE(?, "newsletter"),
            "accept_messages" = COALESCE(?, "accept_messages"),
            "track_contributions" = COALESCE(?, "track_contributions"),
            "ai_opt_out" = COALESCE(?, "ai_opt_out"),
            "extra_skills" = COALESCE(?, "extra_skills"),
            "extra_tools" = COALESCE(?, "extra_tools"),
            "updated" = current_timestamp
        WHERE "user_id" = ?"#,
        update.handle,
        update.first_name,
        update.last_name,
        update.image,
        update.bio,
        update.birthday,
        update.gender,
        update.zipcode,
        update.country,
        update.country_code,
        update.mailing_address,
        update.facebook_url,
        update.twitter_profile,
        update.home_page,
        update.inat_username,
        update.zooniverse_username,
        update.orcid,
        update.degree,
        update.ethnicity,
        update.income,
        update.scientist,
        update.educator,
        update.public,
        update.protected,
        update.accept_emails,
        update.newsletter,
        update.accept_messages,
        update.track_contributions,
        update.ai_opt_out,
        update.extra_skills,
        update.extra_tools,
        user_id_str,
    )
    .execute(db())
    .await?;

    // A protected user is never on a mailing list. Enforced as its own
    // statement, after the fact, so it holds however the account arrived at
    // being protected — subscribing and protecting in the same save, or
    // subscribing a profile that was already protected.
    sqlx::query!(
        r#"UPDATE "users" SET "newsletter" = 0 WHERE "user_id" = ? AND "protected" = 1"#,
        user_id_str,
    )
    .execute(db())
    .await?;

    Ok(())
}

pub async fn public_get(req: &Request) -> Result<PublicProfile, Error> {
    let user_id: String = flexbuffers::from_slice(&req.message.payload)?;

    let row = sqlx::query!(
        r#"SELECT
            "user_id",
            "handle",
            "first_name",
            "last_name",
            "image",
            "bio",
            "facebook_url",
            "twitter_profile",
            "home_page",
            "inat_username",
            "zooniverse_username",
            "orcid",
            "scientist" as "scientist: bool",
            "educator" as "educator: bool",
            "ambassador",
            "public" as "public: bool",
            "protected" as "protected: bool",
            "date_joined"
        FROM "users"
        WHERE "user_id" = ?"#,
        user_id
    )
    .fetch_optional(db())
    .await?
    .ok_or_else(|| anyhow!("User not found: {}", user_id))?;

    let visibility = if row.public && !row.protected {
        PublicProfileVisibility::Public
    } else {
        PublicProfileVisibility::Restricted
    };

    let zooniverse_username = if row.zooniverse_username.is_empty() {
        None
    } else {
        Some(row.zooniverse_username)
    };

    Ok(match visibility {
        PublicProfileVisibility::Public => PublicProfile {
            user_id: row.user_id.as_ulid()?,
            handle: row.handle,
            visibility,
            first_name: row.first_name,
            last_name: row.last_name,
            image: row.image,
            bio: row.bio,
            home_page: row.home_page,
            facebook_url: row.facebook_url,
            twitter_profile: row.twitter_profile,
            inat_username: row.inat_username,
            zooniverse_username,
            orcid: row.orcid,
            scientist: row.scientist,
            educator: row.educator,
            ambassador: row.ambassador,
            date_joined: row.date_joined,
        },
        // The handle and the avatar identify their owner as surely as the
        // name does, so a restricted profile carries neither: consumers
        // that title a page or label a list entry with `handle` would
        // otherwise republish exactly what was withheld below.
        PublicProfileVisibility::Restricted => PublicProfile {
            user_id: row.user_id.as_ulid()?,
            handle: PRIVATE_DISPLAY_NAME.to_string(),
            visibility,
            first_name: String::new(),
            last_name: String::new(),
            image: None,
            bio: None,
            home_page: None,
            facebook_url: None,
            twitter_profile: None,
            inat_username: None,
            zooniverse_username: None,
            orcid: None,
            scientist: false,
            educator: false,
            ambassador: None,
            date_joined: row.date_joined,
        },
    })
}
