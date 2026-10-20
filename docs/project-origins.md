# Project Origins

Most projects on SciStarter are created by hand, by a project manager or
by SciStarter staff filling in the project form. A sizable minority,
though, arrive automatically from partner catalogs, by the partner API
or by our import of the Atlas of Living Australia’s catalog.

None of these paths ever publishes a project. Imported projects wait for
staff review like any other new project.

Every project records where it came from in a field called “origin.”
Projects created by hand say “scistarter,” but so do very old projects
whose origin was never recorded.

## The Partner API

CitSci.org and Anecdata send their projects to us through our API. Each
time they send a project, we either create a record or update the copy
we already have, matched by the project’s GUID. A key may only update a
project its account owns or manages, or one in an exchange the account
runs, unless the account is staff.

Projects from an account marked “automatic affiliate” become affiliates.
A project that names no origin is labeled with the account that sent it,
as “API import by” and the account’s email address. The project
reviewers hear about each new project unless the sending account is set
not to notify them.

This is the busiest path. As of October 2026, CitSci.org has sent us
about 500 projects and Anecdata about 250. A handful of other
organizations have used the API occasionally.

## Atlas of Living Australia

The Atlas of Living Australia (ALA) runs a catalog of citizen science
projects called BioCollect. Every three days we read the whole catalog
and import any projects that are new or have changed since last time.
About 770 projects have come to us this way. When several copies of the
site are running, one of them is chosen to do each import.

ALA’s catalog includes copies of projects from other catalogs, including
about 1,200 of SciStarter’s own, along with CitSci.org projects,
Anecdata projects, and others. We only import the roughly 650 projects
that actually originate at ALA. The others either started here or reach
us by their own routes, and importing them again would create
duplicates.

For the same reason, if an ALA project has exactly the same name as a
project we already list, we skip it. ALA keeps its own records for
well-known projects such as iNaturalist and eBird, and we don’t want a
second copy of those.

When we do import a project, ALA’s version replaces ours, with two
exceptions:

1. If a project manager or staff wrote a field, on the project’s page,
   in the admin, or in the form that created the project, that field is
   protected and the import leaves it alone. The partner API respects
   the same protection.
2. Equipment and “how to join” are only filled in when ours are empty,
   and images are only downloaded when the project has none.

Unless ALA says a project is global, we also use ALA’s map of where the
project takes place to decide which regions it belongs to.

## The Federal Catalog

The old site also once imported projects from citizenscience.gov, a
catalog of federally funded citizen science run by the Wilson Center.
That’s why those projects have the origin “wilsoncenter.” The catalog
lost its funding and no longer exists, so that import was never brought
over here. Its projects arrived through the sync like any others.
