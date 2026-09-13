CREATE TABLE IF NOT EXISTS "work" (
  "id" INTEGER PRIMARY KEY,
  "title" TEXT NOT NULL,
  "filtered_fandom" TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS "series" (
  "id" INTEGER PRIMARY KEY,
  "title" TEXT NOT NULL,
  "begun" DATE NOT NULL,
  "updated" DATE NOT NULL,
  "description" TEXT,
  "num_words" INTEGER NOT NULL,
  "num_works" INTEGER NOT NULL,
  "is_completed" BOOLEAN NOT NULL,
  "num_bookmarks" INTEGER NOT NULL,
  "filtered_fandom" TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS "author" (
  "id" INTEGER PRIMARY KEY,
  "name" TEXT UNIQUE NOT NULL
);

CREATE TABLE IF NOT EXISTS "tag" (
  "id" INTEGER PRIMARY KEY,
  "type" TEXT NOT NULL,
  "name" TEXT UNIQUE NOT NULL
);

CREATE TABLE IF NOT EXISTS "work_author_link" (
	"id" INTEGER PRIMARY KEY,
	"work" INTEGER NOT NULL,
	"author" INTEGER NOT NULL,
	FOREIGN KEY("author") REFERENCES "author"("id"),
	FOREIGN KEY("work") REFERENCES "work"("id")
);

CREATE TABLE IF NOT EXISTS "series_author_link" (
  "id" INTEGER PRIMARY KEY,
  "series" INTEGER NOT NULL,
  "author" INTEGER NOT NULL,
  FOREIGN KEY("author") REFERENCES "author"("id"),
  FOREIGN KEY("series") REFERENCES "series"("id")
);

CREATE TABLE IF NOT EXISTS "work_tag_link" (
	"id" INTEGER PRIMARY KEY,
	"work" INTEGER NOT NULL,
	"tag" INTEGER NOT NULL,
	FOREIGN KEY("tag") REFERENCES "tag"("id"),
	FOREIGN KEY("work") REFERENCES "work"("id")
);

CREATE TABLE IF NOT EXISTS "series_tag_link" (
	"id" INTEGER PRIMARY KEY,
	"series" INTEGER NOT NULL,
	"tag" INTEGER NOT NULL,
	FOREIGN KEY("tag") REFERENCES "tag"("id"),
	FOREIGN KEY("series") REFERENCES "series"("id")
);

CREATE TABLE IF NOT EXISTS "work_series_link" (
	"id" INTEGER PRIMARY KEY,
	"work" INTEGER NOT NULL,
	"series" INTEGER NOT NULL,
  "part_in_series" INTEGER NOT NULL,
	UNIQUE("work", "series")
);

CREATE TABLE IF NOT EXISTS "upload_queue" (
	"id" INTEGER PRIMARY KEY,
  "type" TEXT NOT NULL,
  "device" TEXT NOT NULL,
  "id_to_upload" INTEGER NOT NULL,
  UNIQUE("type", "device", "id_to_upload")
);
