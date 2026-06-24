// Shared social-sharing helpers used by the project/event detail share modal
// and the embeddable exchange project widget.
//
// Instagram has no web link-share endpoint (unlike Facebook/X/LinkedIn, which
// take a URL in a query string). The web-viable path is the OS share sheet:
// on mobile we hand the entity's own uploaded image to navigator.share() (where
// Instagram appears as a target) and copy a caption + link for the user to
// paste. On desktop / unsupported browsers we copy the caption + link and open
// Instagram so the user can paste it into their post.
//
// The trigger element supplies the data:
//   data-share-url     canonical page URL
//   data-share-image   absolute URL of the uploaded image (may be empty)
//   data-share-caption suggested caption (entity name)
window.ssShareToInstagram = async function (btn) {
  const url = btn.dataset.shareUrl;
  const caption = btn.dataset.shareCaption;
  const imageUrl = btn.dataset.shareImage;

  // Optional, page-provided status line; absent on surfaces that don't have one.
  const status = document.getElementById("ig-share-status");
  const say = (msg) => {
    if (status) {
      status.textContent = msg;
      status.hidden = false;
    }
  };

  const captionBlock = [caption, url].filter(Boolean).join("\n\n");
  let copied = false;
  try {
    await navigator.clipboard.writeText(captionBlock);
    copied = true;
  } catch (e) {}

  // Best path: share the image as a file so Instagram opens with it loaded.
  if (imageUrl && navigator.canShare) {
    try {
      const res = await fetch(imageUrl, { mode: "cors" });
      if (res.ok) {
        const blob = await res.blob();
        const type = blob.type || "image/jpeg";
        const ext = (type.split("/")[1] || "jpg").replace("jpeg", "jpg");
        const file = new File([blob], `scistarter-share.${ext}`, { type });
        if (navigator.canShare({ files: [file] })) {
          await navigator.share({ files: [file], text: caption, url });
          say("Caption copied — paste it into your Instagram post.");
          return;
        }
      }
    } catch (e) {
      if (e && e.name === "AbortError") return;
    }
  }

  // Next best: plain share sheet (still lists Instagram on mobile).
  if (navigator.share) {
    try {
      await navigator.share({ text: caption, url });
      say("Caption copied — paste it into your Instagram post.");
      return;
    } catch (e) {
      if (e && e.name === "AbortError") return;
    }
  }

  // Desktop fallback: caption + link already on the clipboard; open Instagram.
  say(
    copied
      ? "Link copied. Open Instagram and paste it into your post."
      : "Open Instagram to share this link from your post."
  );
  window.open("https://www.instagram.com/", "_blank", "noopener");
};
