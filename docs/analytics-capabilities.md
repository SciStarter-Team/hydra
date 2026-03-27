# SciStarter Analytics Capabilities

This document describes the analytics data available or obtainable from
the SciStarter platform, organized by topic.

## Site Traffic & Visitor Metrics

- **Page views** with full URL path and timestamp
- **Unique visitors** by persistent device identifier
- **Sessions** tracked via interaction identifiers (1-hour inactivity
  expiry), enabling session count and session duration metrics
- **Bounce rate** — single-page sessions identifiable from interaction
  IDs with only one associated page view
- **Return visitor rate** — repeat device IDs across multiple days or
  interaction sessions
- **Authenticated vs. anonymous traffic** split — user ID presence
  distinguishes logged-in visitors
- **Pages per session** — count of page views grouped by interaction ID
- **Time-of-day and day-of-week traffic patterns** — derivable from
  page view timestamps
- **Traffic trends** — weekly, monthly, and custom date range
  aggregations

## Go Link Traffic & Visitor Metrics

Go Links permit collecting analytics as users traverse between a
source page and a link target

- **Page views** with full URL path and timestamp
- **Unique visitors** by persistent device identifier
- **Sessions** tracked via interaction identifiers (1-hour inactivity
  expiry), enabling session count and session duration metrics
- **Return visitor rate** — repeat device IDs across multiple days or
  interaction sessions
- **Authenticated vs. anonymous traffic** split — user ID presence
  distinguishes logged-in visitors
- **Time-of-day and day-of-week traffic patterns** — derivable from
  page view timestamps
- **Traffic trends** — weekly, monthly, and custom date range
  aggregations

## User Acquisition & Growth

- **Total registered users** and **verified users**
- **Signup rate** — daily, weekly, and monthly new user counts
- **Cumulative growth curve** — running total of users over time
- **Verification rate** — proportion of signups who verify their email
- **User geographic distribution** — country-level breakdown of
  verified users, based on IP geolocation at signup and user-provided
  zip codes
- **Signup source attribution** — when go links with a `program`
  parameter are used as landing page URLs, the program ID is captured
  and can be correlated with subsequent signups in the same
  interaction session. UTM parameters in landing page URLs can also be
  used for attribution if go links or campaign URLs include them
  (requires go links or UTM parameters in campaign URLs)
- **User tags** — segmentation by tags (e.g., "evolveme",
  "libraries-hubs") assigned to users

## Referral & Attribution

- **Referring domains** — HTTP Referer header captured on every page
  view, enabling top referrer rankings and referral traffic volume, if
  the browser reports a referrer (referrer reporting is becoming rarer)
- **Referral paths** — full referrer URLs available for detailed
  analysis, if the browser reports a referrer
- **Go link attribution** — go links (`/go/{slug}`) track all
  accesses with the originating slug, enabling campaign-specific
  traffic measurement
- **Program attribution** — go links accept an optional `program`
  query parameter, linking traffic to specific programs or campaigns
- **UTM campaign tracking** — UTM parameters (utm_source,
  utm_medium, utm_campaign, utm_content, utm_term) present in
  landing page query strings can be extracted and analyzed (requires
  UTM parameters in campaign URLs; query strings are already captured
  on every page view)
- **External link click attribution** — all outbound links on the
  site are automatically rewritten to pass through the tracking
  system, capturing which page the user was on and which external URL
  they clicked
- **First-touch vs. last-touch attribution** — the combination of
  referrer, go link program ID, and page view sequence within an
  interaction session allows reconstruction of the attribution chain

## Search & Discovery

- **Search query terms** — the text users search for in the project
  finder
- **Search filter usage** — which filters are applied (topic,
  activity, audience, location type, misc filters like
  "has_classroom_materials" or "is_affiliate")
- **Search result counts** — number of results returned per query
- **Location-based search patterns** — whether users search by
  location, and the geographic distribution of location-based
  searches
- **Search-to-engagement rate** — proportion of searches that lead
  to a project page view or clickthrough
- **Zero-result searches** — queries that return no results,
  indicating content gaps
- **Popular search terms** — ranked list of most frequent search
  queries
- **Filter combination analysis** — which filter combinations are
  most commonly used together
- **Autocomplete usage** — searches that use the autocomplete/FTS
  suggestions vs. free-form queries

## Content Performance

- **Page view rankings** — most-viewed pages by path, with date
  range filtering
- **Project popularity** — project rankings by views,
  clickthroughs, or participation counts
- **Content engagement rate** — ratio of page views to
  clickthroughs or participation actions per project/event
- **Trending content** — projects or pages with accelerating view
  counts over recent periods
- **Content performance by category** — views and engagement
  segmented by project topic, activity type, audience, field of
  science, or geographic scope
- **Event performance** — event page views and engagement metrics,
  segmentable by event type and tags
- **Project status distribution** — breakdown of active, complete,
  hiatus, and periodically active projects

## Outbound Clickthrough Analytics

- **Total clickthroughs** with weekly aggregation and date range
  filtering
- **Clickthrough by link type** — "golink" vs. "external_link"
  segmentation
- **Top clicked destinations** — ranking of external URLs by click
  volume
- **Clickthrough source pages** — which site pages generate the
  most outbound clicks
- **Program-attributed clickthroughs** — clicks associated with
  specific programs via go link program parameter (requires go links
  with program parameter)
- **Go link performance** — per-slug statistics including total
  views (all-time), views in date range, daily view chart, and
  recent access log
- **Click-to-participation conversion** — proportion of external
  clickthroughs that are followed by a participation record for the
  same user and project

## Participation & Contributions

- **Total participation events** with weekly aggregation
- **Participation by origin** — segmented by source: web, form,
  affiliate, incidental, admin, or Google Form
- **Participation duration** — hours spent per participation event
- **Participation magnitude** — intensity/count metrics per event
- **Participation fidelity** — quality score of participation data
- **Participation by project** — per-project participation counts
  and trends
- **Participation geography** — location of participation events
  (when user-provided location is available)
- **Task completion tracking** — specific tasks completed within
  projects (e.g., Articulate course completions)
- **Repeat participation rate** — users who participate in the same
  project multiple times
- **Cross-project participation** — users who participate in
  multiple projects
- **Top contributors** — users ranked by participation volume
- **Participation trends** — temporal patterns in contribution
  activity
- **Acts of Science** — dedicated tracking for citizen science month
  contributions, including contributor demographics, organizations,
  and project associations

## User Engagement & Retention

- **Project joins** — users joining projects, with active/inactive
  state tracking
- **Project bookmarks** — saves/unsaves with timestamps
- **Engagement funnel** — progression from page view to
  clickthrough to join to participation, trackable per user and per
  project
- **User retention cohorts** — return visit patterns grouped by
  signup week/month using device ID and user ID
- **Session depth distribution** — histogram of pages per session
  across all users
- **Feature adoption** — which site features (search, bookmarks,
  joins, forum) each user segment engages with
- **Lapsed user identification** — users who were active but have
  not returned within a defined period
- **Badge earning rates** — frequency and distribution of badge
  achievements across the user base
- **Engagement by user tag** — activity metrics segmented by user
  tag for program-specific analysis

## Email Marketing Analytics

- **Emails sent** per campaign (sending ID)
- **Open rate** — tracked via embedded pixel, per campaign and
  overall
- **Bounce rate** — permanent, temporary, and undetermined bounces,
  per campaign
- **Complaint rate** — spam complaints per campaign
- **Unsubscribe rate** — per campaign and cumulative
- **Email deliverability** — sent minus bounces
- **Campaign performance comparison** — side-by-side metrics across
  sending IDs
- **Email-to-action conversion** — correlation between email opens
  and subsequent site visits or participation within the same user's
  interaction timeline
- **List health metrics** — cumulative bounces, unsubscribes, and
  complaint rates across the subscriber base

## Exchange Analytics

- **Exchange visits** — distinct interaction sessions viewing the
  exchange home page
- **Unique exchange visitors** — distinct devices accessing the
  exchange
- **Project views within exchange** — page views for projects in
  the exchange context
- **Exchange search activity** — searches performed within an
  exchange
- **Project saves/votes** — bookmarking and voting activity within
  exchanges (when enabled)
- **Top exchange projects** — ranked by views and saves
- **Year-over-year exchange engagement** — metrics filterable by
  year
- **Exchange-to-participation conversion** — proportion of exchange
  project views that lead to participation

## Geographic Analytics

- **Visitor geolocation** — country, region, city, and coordinates
  resolved from IP address on every page view and clickthrough
- **User home location** — self-reported zip code and optional
  coordinates
- **Participation geography** — where participation events occur
- **Country-level user distribution** — verified user counts by
  country with country code
- **Regional engagement patterns** — traffic volume and engagement
  rates by geographic area
- **Location-based search demand** — geographic distribution of
  location-filtered searches
- **Content relevance by region** — which projects and topics are
  most engaged with in different geographies

## User Journey & Conversion Paths

- **Page view sequences** — ordered page visits within an
  interaction session, reconstructable from timestamps and
  interaction IDs
- **Entry pages** — first page viewed in each interaction session
- **Exit pages** — last page viewed before session expiry
- **Common navigation paths** — frequently traversed page sequences
- **Conversion paths** — the sequence of pages visited before a
  participation event, join, or signup
- **Attribution chains** — referrer → landing page → navigation →
  conversion, traceable per interaction session
- **Multi-session journeys** — longitudinal user journeys across
  sessions using device ID or user ID
- **Funnel drop-off analysis** — at which stage users abandon
  the path from discovery to participation

## Platform Integration Analytics

- **Zooniverse integration** — bridge tables linking Zooniverse
  user IDs and project IDs to SciStarter entities, enabling
  cross-platform participation tracking
- **iNaturalist integration** — bridge tables linking iNaturalist
  project IDs to SciStarter entities
- **Affiliate participation** — participation events originating
  from affiliate sources, trackable by entity

## Administrative & Operational Metrics

- **Forum activity** — topic and post creation rates, moderation
  actions
- **Content moderation volume** — moderator actions over time
- **File download tracking** — go links pointing to tracked files
  capture download counts
- **System activity feed** — real-time log of all significant
  actions (signups, logins, project creation, etc.) with user
  attribution
- **Data export** — CSV exports of users, projects, and events for
  offline analysis
