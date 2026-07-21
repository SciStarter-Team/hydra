#!/usr/bin/env python3

import json
from hashlib import sha256
from urllib.request import Request, urlopen
from urllib.parse import urlencode
from os import environ


def record_participation(email, project_slug):
    """Record participation for the specified user in the specified project

    This will appear to succeed, regardless of whether the user is
    actually a SciStarter user or not. However, in that case this API
    call is a no-op. It only reports an error if the request is
    incorrect in some way.

    If the email address does *not* belong to a SciStarter user, all
    we've received is an opaque hash value, which preserves the user's
    privacy; we have no way of reversing the hashing process to
    discover the email.

    The project_slug parameter should contain the textual unique
    identifier of the project. It is easily accesible from the project
    URL. In the URL https://scistarter.org/airborne-walrus-capture the
    slug is the string airborne-walrus-capture

    """

    hashed = sha256(email.lower().encode("utf8")).hexdigest()

    req = Request(
        method="POST",
        url="https://scistarter.org/api/participation/hashed/" + project_slug + "?key=" + environ["SCISTARTER_API_KEY"],
        data=urlencode(
            {
                "hashed": hashed,
                "type": "classification",  # other options: 'collection', 'signup'
                "duration": 31,  # Seconds the user spent participating, or an estimate
            }
        ).encode("utf8"),
    )

    r = urlopen(req)

    if r.status != 200:
        raise Exception(r.status, r.reason)

    return json.loads(r.read())


if __name__ == "__main__":
    print(record_participation(input("Email: "), input("Project slug: ")))
