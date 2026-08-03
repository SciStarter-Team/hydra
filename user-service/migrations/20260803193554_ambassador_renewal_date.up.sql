-- The last orrery AmbassadorApplication column without a home here. The staff
-- grid filters on it (renewal due / not due), and the orrery sync carries it.
ALTER TABLE "ambassador_applications" ADD COLUMN "renewal_date" text;
