use anyhow::Error;
use async_nats::service::Request;
use common::{
    message::email::{EmailRecipient, SendEmailRequest, SendEmailResponse, noreply_reply_to},
    message::user::{
        Attribution, BasicInfo, CompleteSignup, CompleteSignupResponse, DeferSignup,
        DeferSignupOutcome, Login, PendingTransition, Signup, SnmProfile, UserAndBadge,
        normalize_tag, signup_needs_protection, welcome_trigger_for_signup,
    },
    service::{TriggerEvent, log_activity, request, send, trigger_email},
    shared::{db, hash_identifier, hash_identifier_capfirst, hash_identifier_lower},
    timestring::TimeString,
    ulid::AsUlid,
};
use ulid::Ulid;

/// How many deferred signups a single IP can generate per hour. Set high
/// enough that a classroom behind one NAT (exactly the population most
/// likely to have Turnstile blocked) isn't locked out.
const PENDING_SIGNUP_IP_HOURLY_LIMIT: i64 = 30;

#[derive(Debug)]
struct UserWithPassword {
    user_id: String,
    handle: String,
    password: String,
    is_staff: i64,
    is_superuser: i64,
    public: i64,
    protected: i64,
    point: Option<String>,
}

/// Sign the current session up for a new account
pub async fn signup(req: &Request) -> Result<Option<BasicInfo>, Error> {
    let form: Signup = flexbuffers::from_slice(&req.message.payload)?;

    let password = if form.disabled_password {
        // Store Django-style disabled password marker directly
        "!".to_string()
    } else {
        // Hash the user's password
        djangohashers::make_password(&form.password)
    };

    match create_account(form, password, false).await {
        Ok(basic_info) => Ok(Some(basic_info)),
        // The email address already belongs to an account. Caught by the
        // insert rather than checked beforehand, so that two signups racing for
        // one address cannot both get past the check.
        Err(err) if is_unique_violation(&err) => Ok(None),
        Err(err) => Err(err),
    }
}

/// Whether an error is the database refusing a row that duplicates a unique
/// key, which for account creation means the email address is already in use.
fn is_unique_violation(err: &Error) -> bool {
    err.downcast_ref::<sqlx::Error>()
        .and_then(|e| e.as_database_error())
        .is_some_and(|e| e.is_unique_violation())
}

/// Insert the user, primary email, and tags, then fire the welcome email.
/// Shared by the immediate signup lane and the deferred (email-completion)
/// lane; `password` must already be hashed. `email_verified` marks accounts
/// whose owner has proven control of the inbox by clicking a completion
/// link, which outweighs the signals that would otherwise leave the account
/// unverified.
async fn create_account(
    form: Signup,
    password: String,
    email_verified: bool,
) -> Result<BasicInfo, Error> {
    let user_id = Ulid::new();
    let user_id_str = user_id.to_string();
    let handle = form.handle;
    let email = form.email;
    let zipcode = form.zipcode;
    let point = form.point;
    let badgeclass = form.badgeclass;
    let attribution = form.attribution;
    let pending_transitions = form.pending_transitions;
    let join_project = form.join_project;

    // Some signup contexts identify a cohort that needs protecting. The rule
    // lives here rather than in a frontend handler so that every lane —
    // immediate, passkey, and email-completion — obeys it, and so the
    // account is protected from the row that creates it.
    let protected = form.protected || signup_needs_protection(&form.tags);

    // A protected user is never added to a mailing list.
    let newsletter = form.newsletter && !protected;

    // Nor is their profile public. The column defaults to 1, and every reader
    // reports `public && !protected` anyway, so leaving it to the default
    // would store a yes that nobody is ever shown -- and that takes effect the
    // day protection is lifted.
    let public = !protected;

    let signup_tags = crate::constant_contact::encode_tags(&form.tags);
    let date_joined = TimeString::now().to_string();

    let mut tx = db().begin().await?;

    // We're letting users and emails start as verified, for now,
    // unless some criteria are met which make them less trusted
    // initially.

    let mut verified = true;

    if form.tags.iter().any(|s| normalize_tag(s) == "evolveme") {
        verified = false;
    }

    if email_verified {
        verified = true;
    }

    sqlx::query!(
        r#"insert into "users" (
          "user_id",
          "date_joined",
          "handle",
          "primary_email",
          "verified",
          "password",
          "newsletter",
          "zipcode",
          "protected",
          "public",
          "point",
          -- Every tag on a new account arrived with the person, so the whole
          -- set is recorded as having come from a signup context. That is what
          -- lets the Constant Contact mirror carry a campaign tag it could not
          -- have named in advance.
          "signup_tags",
          -- Flagged for the Constant Contact sweep from the first moment. The
          -- sweep only considers subscribers, so this is inert for anyone who
          -- did not ask for the newsletter, and it costs one row update rather
          -- than a new account sitting unknown to them until something else
          -- happens to touch it.
          "constant_contact_dirty",
          "referer",
          "origin",
          "utm_source",
          "utm_medium",
          "utm_campaign",
          "utm_term",
          "utm_content"
        )
        values (
          ?1,
          ?12,
          ?3,
          ?2,
          ?9,
          ?4,
          ?5,
          ?6,
          ?7,
          ?11,
          SetSRID(GeomFromGeoJSON(?8), 4326),
          ?10,
          1,
          ?13,
          ?14,
          ?15,
          ?16,
          ?17,
          ?18,
          ?19
        )"#,
        user_id_str,
        email,
        handle,
        password,
        newsletter,
        zipcode,
        protected,
        point,
        verified,
        signup_tags,
        public,
        date_joined,
        attribution.referer,
        attribution.origin,
        attribution.utm_source,
        attribution.utm_medium,
        attribution.utm_campaign,
        attribution.utm_term,
        attribution.utm_content,
    )
    .execute(&mut *tx)
    .await?;

    let hashed = hash_identifier(&email);
    let hashed_lower = hash_identifier_lower(&email);
    let hashed_capfirst = hash_identifier_capfirst(&email);

    sqlx::query!(
        r#"insert into "emails" (
          "user_id",
          "email",
          "verified",
          "hash",
          "hash_lower",
          "hash_capfirst"
        )
        values (
          ?,
          ?,
          ?,
          ?,
          ?,
          ?
        )"#,
        user_id_str,
        email,
        verified,
        hashed,
        hashed_lower,
        hashed_capfirst,
    )
    .execute(&mut *tx)
    .await?;

    for tag in form.tags.iter() {
        // The same spelling the account's `signup_tags` will hold, so that the
        // two copies of a signup tag are the one string.
        let tag = normalize_tag(tag);

        if tag.is_empty() {
            continue;
        }

        sqlx::query!(
            r#"insert into "user_tags" (
              "user_id",
              "tag"
            )
            values (
              ?,
              ?
            )
            on conflict ("user_id", "tag") do nothing"#,
            user_id_str,
            tag,
        )
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    if point.is_some() {
        // Names the state from the location just recorded, which is what lets
        // staff build a regional segment. Off the response path: it may mean a
        // geocoding call.
        crate::constant_contact::derive_state_in_background(user_id_str.clone());
    }

    if newsletter {
        // Subscribing is what creates their Constant Contact record; the sweep
        // fills in the details it holds within twelve hours. Off the response
        // path, since nobody should wait on that service to finish signing up.
        crate::constant_contact::subscribe_in_background(user_id_str.clone(), email.clone());
    }

    // Staked out by the tracked go link the visitor arrived through. Granted
    // here rather than by the caller so that every lane obeys it — immediate,
    // passkey, and email-completion — and so it cannot be lost between the
    // account being created and the caller getting a chance to act.
    //
    // Not fatal: the account is committed by this point, so a badge that fails
    // to send must not turn a completed signup into an error. Same reasoning
    // as the welcome email below.
    if let Some(badge) = badgeclass {
        if let Err(e) = send("user.badges.grant", UserAndBadge { user_id, badge }).await {
            eprintln!("Failed to grant staked-out badge to {user_id}: {e}");
        }
    }

    // Some signup contexts identify a cohort that ran a training before
    // enrolling, as the signup tag grants record. Credited here for the reason protection is decided
    // here -- every lane obeys it -- and non-fatally for the reason the badge
    // above is: the account is committed by this point, so a credit that fails
    // must not turn a completed signup into an error. The cohort backfill
    // catches whoever it misses.
    if let Err(e) = crate::signup_tag_grants::credit(&user_id_str, &form.tags).await {
        eprintln!("Failed to credit the signup cohort for {user_id}: {e}");
    }

    // Training steps the visitor earned before they had an account -- a
    // tracked go link followed signed out. Recorded here for the reasons the
    // badge and the cohort credit are: every lane obeys it, and non-fatally,
    // since the account is committed by now. Recorded in the order they were
    // earned, which is the order a machine would have taken them in.
    for PendingTransition { workflow, step } in pending_transitions {
        if let Err(e) = crate::workflow::apply_step(&user_id_str, workflow, &step).await {
            eprintln!(
                "Failed to record the {} step {step:?} held for {user_id}: {e}",
                workflow.slug()
            );
        }
    }

    // The project the visitor signed up from. Joined here for the reasons
    // the badge is granted here: every lane obeys it, and non-fatally.
    if let Some(project_id) = join_project {
        if let Err(e) = crate::project_joins::record_join(user_id, &project_id).await {
            eprintln!("Failed to join {user_id} to {project_id} at signup: {e}");
        }
    }

    log_activity(None, "signup", user_id, None, None).await.ok();

    // Some cohorts are welcomed in their own words. The tag decides which,
    // as it does in orrery, and the trigger name is the only thing that
    // differs -- a cohort with no welcome of its own gets the generic one.
    let welcome = welcome_trigger_for_signup(&form.tags).unwrap_or(TriggerEvent::UserWelcome);

    trigger_email(
        welcome,
        user_id,
        Some(&[("handle".to_string(), handle.clone())]),
    )
    .await
    .ok();

    // An account that starts out unverified is asked to confirm its address
    // straight away, as orrery asks. Not fatal, for the reason the welcome
    // email is not: the account exists by now, and the banner it is shown
    // offers to send the email again.
    if !verified {
        if let Err(e) = crate::email_verification::send_to_new_account(user_id, &email).await {
            eprintln!("Failed to send the confirmation email to {user_id}: {e}");
        }
    }

    Ok(BasicInfo {
        user_id: user_id,
        handle,
        is_staff: false,
        is_superuser: false,
        public: true,
        protected,
        needs_point: point.is_none(),
    })
}

async fn send_completion_email(email: &str, code: &str) -> Result<(), Error> {
    let complete_url = format!("https://scistarter.org/complete-signup?code={code}");

    request::<SendEmailResponse>(
        "emailer.send",
        SendEmailRequest {
            template_html: format!(
                "<p>You're almost there! To finish creating your SciStarter \
                 account, click the link below:</p>\
                 <p><a href=\"{complete_url}\">Complete my SciStarter signup</a></p>\
                 <p>This link expires in 7 days. If you didn't try to create a \
                 SciStarter account, you can safely ignore this email.</p>"
            ),
            subject: "Finish creating your SciStarter account".to_string(),
            recipients: vec![EmailRecipient::Email(email.to_string())],
            data_sets: vec![],
            additional_data: None,
            reply_to: noreply_reply_to(true),
            no_unsubscribe: false,
        },
    )
    .await?;

    Ok(())
}

/// Store (or refresh) a signup that Turnstile could not vouch for, and email
/// its completion link. No users row is created until the link is clicked,
/// so a bot without a real inbox gets nothing.
pub async fn defer_signup(req: &Request) -> Result<DeferSignupOutcome, Error> {
    let defer: DeferSignup = flexbuffers::from_slice(&req.message.payload)?;
    let form = defer.signup;

    // Sweep expired rows opportunistically so the table cannot grow without
    // bound even if the periodic cleanup never runs.
    sqlx::query!(
        r#"delete from "pending_signups" where datetime("created") < datetime('now', '-7 days')"#
    )
    .execute(db())
    .await?;

    let email_exists = sqlx::query_scalar!(
        r#"select count(*) from "emails" where "email" = ?"#,
        form.email
    )
    .fetch_one(db())
    .await?;

    if email_exists > 0 {
        return Ok(DeferSignupOutcome::AlreadyRegistered);
    }

    let ip_address = defer.ip_address.unwrap_or_default();

    let recent_from_ip = sqlx::query_scalar!(
        r#"select count(*) from "pending_signups"
           where "ip_address" = ? and datetime("email_sent") > datetime('now', '-1 hour')"#,
        ip_address
    )
    .fetch_one(db())
    .await?;

    if recent_from_ip >= PENDING_SIGNUP_IP_HOURLY_LIMIT {
        eprintln!("Pending-signup rate limit tripped for {ip_address}");
        return Ok(DeferSignupOutcome::RateLimited);
    }

    let email_lower = form.email.to_lowercase();
    let password = djangohashers::make_password(&form.password);
    let tags = form.tags.join(",");
    let badgeclass = form.badgeclass.map(|badge| badge.badgeclass().to_string());
    let pending_transitions = serde_json::to_string(&form.pending_transitions)?;
    let attribution = &form.attribution;
    let now = TimeString::now().to_string();

    let existing_code = sqlx::query_scalar!(
        r#"select "code" from "pending_signups" where "email_lower" = ?"#,
        email_lower
    )
    .fetch_optional(db())
    .await?;

    match existing_code {
        Some(code) => {
            let within_cooldown = sqlx::query_scalar!(
                r#"select count(*) from "pending_signups"
                   where "email_lower" = ? and datetime("email_sent") > datetime('now', '-15 minutes')"#,
                email_lower
            )
            .fetch_one(db())
            .await?;

            // A retry refreshes the stored payload either way, but the
            // completion email is only resent outside the cooldown, so the
            // form cannot be used to flood an inbox.
            if within_cooldown == 0 {
                sqlx::query!(
                    r#"update "pending_signups" set
                       "email" = ?, "handle" = ?, "password" = ?, "newsletter" = ?,
                       "zipcode" = ?, "protected" = ?, "point" = ?, "tags" = ?,
                       "badgeclass" = ?, "next" = ?, "ip_address" = ?, "email_sent" = ?,
                       "referer" = ?, "origin" = ?, "utm_source" = ?, "utm_medium" = ?,
                       "utm_campaign" = ?, "utm_term" = ?, "utm_content" = ?,
                       "pending_transitions" = ?, "join_project" = ?
                       where "email_lower" = ?"#,
                    form.email,
                    form.handle,
                    password,
                    form.newsletter,
                    form.zipcode,
                    form.protected,
                    form.point,
                    tags,
                    badgeclass,
                    defer.next,
                    ip_address,
                    now,
                    attribution.referer,
                    attribution.origin,
                    attribution.utm_source,
                    attribution.utm_medium,
                    attribution.utm_campaign,
                    attribution.utm_term,
                    attribution.utm_content,
                    pending_transitions,
                    form.join_project,
                    email_lower,
                )
                .execute(db())
                .await?;

                println!("Deferring signup to email completion: {}", form.email);
                send_completion_email(&form.email, &code).await?;
            } else {
                sqlx::query!(
                    r#"update "pending_signups" set
                       "email" = ?, "handle" = ?, "password" = ?, "newsletter" = ?,
                       "zipcode" = ?, "protected" = ?, "point" = ?, "tags" = ?,
                       "badgeclass" = ?, "next" = ?, "ip_address" = ?,
                       "referer" = ?, "origin" = ?, "utm_source" = ?, "utm_medium" = ?,
                       "utm_campaign" = ?, "utm_term" = ?, "utm_content" = ?,
                       "pending_transitions" = ?, "join_project" = ?
                       where "email_lower" = ?"#,
                    form.email,
                    form.handle,
                    password,
                    form.newsletter,
                    form.zipcode,
                    form.protected,
                    form.point,
                    tags,
                    badgeclass,
                    defer.next,
                    ip_address,
                    attribution.referer,
                    attribution.origin,
                    attribution.utm_source,
                    attribution.utm_medium,
                    attribution.utm_campaign,
                    attribution.utm_term,
                    attribution.utm_content,
                    pending_transitions,
                    form.join_project,
                    email_lower,
                )
                .execute(db())
                .await?;

                println!(
                    "Deferred signup refreshed within cooldown, not resending: {}",
                    form.email
                );
            }
        }
        None => {
            let code = crate::secret_code::generate(crate::secret_code::CODE_BYTES);

            sqlx::query!(
                r#"insert into "pending_signups" (
                   "code", "email", "email_lower", "handle", "password",
                   "newsletter", "zipcode", "protected", "point", "tags",
                   "badgeclass", "next", "ip_address", "created", "email_sent",
                   "referer", "origin", "utm_source", "utm_medium",
                   "utm_campaign", "utm_term", "utm_content", "pending_transitions",
                   "join_project"
                ) values (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
                code,
                form.email,
                email_lower,
                form.handle,
                password,
                form.newsletter,
                form.zipcode,
                form.protected,
                form.point,
                tags,
                badgeclass,
                defer.next,
                ip_address,
                now,
                now,
                attribution.referer,
                attribution.origin,
                attribution.utm_source,
                attribution.utm_medium,
                attribution.utm_campaign,
                attribution.utm_term,
                attribution.utm_content,
                pending_transitions,
                form.join_project,
            )
            .execute(db())
            .await?;

            println!("Deferring signup to email completion: {}", form.email);
            send_completion_email(&form.email, &code).await?;
        }
    }

    Ok(DeferSignupOutcome::Deferred)
}

/// Redeem an emailed completion code: create the deferred account and log
/// the completion. Returns None when the code is unknown, expired, or the
/// account already exists.
pub async fn complete_signup(req: &Request) -> Result<Option<CompleteSignupResponse>, Error> {
    let complete: CompleteSignup = flexbuffers::from_slice(&req.message.payload)?;

    let row = sqlx::query!(
        r#"select "email", "handle", "password", "newsletter", "zipcode",
                  "protected", "point", "tags", "badgeclass", "next",
                  "referer", "origin", "utm_source", "utm_medium",
                  "utm_campaign", "utm_term", "utm_content", "pending_transitions",
                  "join_project"
           from "pending_signups"
           where "code" = ? and datetime("created") >= datetime('now', '-7 days')"#,
        complete.code
    )
    .fetch_optional(db())
    .await?;

    let Some(row) = row else {
        // Unknown or expired; clear any expired row still bearing this code.
        sqlx::query!(
            r#"delete from "pending_signups" where "code" = ?"#,
            complete.code
        )
        .execute(db())
        .await?;

        return Ok(None);
    };

    let mut tags: Vec<String> = row
        .tags
        .split(',')
        .filter(|tag| !tag.is_empty())
        .map(str::to_string)
        .collect();

    // Marks accounts that arrived through the deferred lane, so the rate of
    // Turnstile failures among real humans can be measured.
    tags.push("signup-turnstile-fallback".to_string());

    let email = row.email.clone();

    let form = Signup {
        handle: row.handle,
        // The stored password is already hashed and passed separately.
        password: String::new(),
        email: row.email,
        newsletter: row.newsletter != 0,
        zipcode: row.zipcode,
        protected: row.protected != 0,
        point: row.point,
        tags,
        disabled_password: false,
        // Staked out before the completion email was sent; the session that
        // carried it is long gone by the time the link is clicked.
        badgeclass: row
            .badgeclass
            .as_deref()
            .and_then(common::badge::Badge::from_badgeclass),
        // Likewise captured when the visitor arrived, not when they click.
        attribution: Attribution {
            referer: row.referer,
            origin: row.origin,
            utm_source: row.utm_source,
            utm_medium: row.utm_medium,
            utm_campaign: row.utm_campaign,
            utm_term: row.utm_term,
            utm_content: row.utm_content,
        },
        // Held on the row for the reason the badge is. A row written before
        // the column existed reads as holding none, via the column default.
        pending_transitions: serde_json::from_str(&row.pending_transitions).unwrap_or_else(
            |error| {
                eprintln!("Unreadable held steps on a pending signup, dropping them: {error}");
                Vec::new()
            },
        ),
        join_project: row.join_project,
    };

    let basic_info = match create_account(form, row.password, true).await {
        Ok(info) => info,
        Err(err) => {
            if is_unique_violation(&err) {
                // The account came into being some other way after this
                // signup was deferred, so the pending record is obsolete.
                sqlx::query!(
                    r#"delete from "pending_signups" where "code" = ?"#,
                    complete.code
                )
                .execute(db())
                .await?;

                return Ok(None);
            }

            return Err(err);
        }
    };

    sqlx::query!(
        r#"delete from "pending_signups" where "code" = ?"#,
        complete.code
    )
    .execute(db())
    .await?;

    println!("Deferred signup completed: {email}");

    Ok(Some(CompleteSignupResponse {
        basic_info,
        next: row.next,
    }))
}

pub async fn login(req: &Request) -> Result<Option<BasicInfo>, Error> {
    let form: Login = flexbuffers::from_slice(&req.message.payload)?;

    // Look up user by email using query_as
    let user_record = sqlx::query_as!(
        UserWithPassword,
        r#"select u.user_id, u.handle, u.password, u.is_staff, u.is_superuser, u.public, u.protected,
           AsText(u.point) as "point: String"
           from users u
           join emails e on u.user_id = e.user_id
           where e.email = ?
           limit 1"#,
        form.email
    )
    .fetch_optional(db())
    .await?;

    if let Some(user) = user_record {
        // Verify password
        if djangohashers::check_password(&form.password, &user.password).unwrap_or(false) {
            // Log successful login
            let user_id = user.user_id.as_ulid().ok();
            log_activity(None, "login", user_id, None, None).await.ok();

            // Return BasicInfo
            Ok(Some(BasicInfo {
                user_id: user.user_id.as_ulid()?,
                handle: user.handle,
                is_staff: user.is_staff != 0,
                is_superuser: user.is_superuser != 0,
                public: user.public != 0,
                protected: user.protected != 0,
                needs_point: user.point.as_ref().map_or(true, |p| p.trim().is_empty()),
            }))
        } else {
            // Log failed login attempt
            let user_id = user.user_id.as_ulid().ok();
            log_activity(None, "login-failed", user_id, None, None)
                .await
                .ok();
            Ok(None)
        }
    } else {
        // User not found
        Ok(None)
    }
}

/// Verify credentials for the Science Near Me login bridge and, on success,
/// return the user's profile fields. Returns None when authentication fails
/// (the frontend turns that into a 404, matching orrery). The transport is
/// encrypted end-to-end at the frontend; this handler only sees plaintext
/// credentials over the internal NATS bus.
pub async fn snm_login(req: &Request) -> Result<Option<SnmProfile>, Error> {
    let form: Login = flexbuffers::from_slice(&req.message.payload)?;

    let user = sqlx::query!(
        r#"SELECT u.user_id, u.handle, u.password, u.first_name, u.last_name,
            u.zipcode, u.newsletter, u.primary_email
           FROM users u
           JOIN emails e ON u.user_id = e.user_id
           WHERE e.email = ?
           LIMIT 1"#,
        form.email
    )
    .fetch_optional(db())
    .await?;

    let Some(user) = user else {
        return Ok(None);
    };

    if !djangohashers::check_password(&form.password, &user.password).unwrap_or(false) {
        return Ok(None);
    }

    let user_id = user.user_id.clone();
    log_activity(None, "snm-login", user_id.as_ulid().ok(), None, None)
        .await
        .ok();

    // All of the user's emails, primary first.
    let mut emails = vec![user.primary_email.clone()];
    let others = sqlx::query_scalar!("SELECT email FROM emails WHERE user_id = ?", user_id)
        .fetch_all(db())
        .await?;
    for email in others {
        if !emails.contains(&email) {
            emails.push(email);
        }
    }

    Ok(Some(SnmProfile {
        username: Some(user.handle).filter(|h| !h.is_empty()),
        first_name: user.first_name,
        last_name: user.last_name,
        emails,
        zip_code: user.zipcode.filter(|z| !z.is_empty()),
        newsletter: user.newsletter != 0,
    }))
}
