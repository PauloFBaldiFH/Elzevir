-- SPDX-License-Identifier: GPL-3.0-or-later
-- Independently authored, fictional PostgreSQL COPY fixture. Read as data.
CREATE TABLE descriptions (id text PRIMARY KEY, title text);
CREATE TABLE units (id text PRIMARY KEY, description text REFERENCES descriptions, accession text);
CREATE TABLE registrations (id text PRIMARY KEY, card text);
CREATE TABLE headings (id text PRIMARY KEY, label text, qualifier text);
CREATE TABLE contributions (description text, heading text, role text);
CREATE TABLE movements (id text PRIMARY KEY, registration text, unit text, returned date);
CREATE TABLE operators (id text PRIMARY KEY);
COPY descriptions (id,title) FROM stdin;
01	Volume sintético
02	Outra edição
\.
COPY units (id,description,accession) FROM stdin;
01	01	000010
02	01	000011
03	02	000012
\.
COPY registrations (id,card) FROM stdin;
01	000007
02	000008
\.
COPY headings (id,label,qualifier) FROM stdin;
A1	Nome de teste	1900-1950
A2	Nome de teste	1960-
\.
COPY contributions (description,heading,role) FROM stdin;
01	A1	aut
02	A9	ill
\.
COPY movements (id,registration,unit,returned) FROM stdin;
M1	01	02	\N
M2	02	01	2001-01-04
M3	99	03	\N
\.
COPY operators (id) FROM stdin;
01
02
\.
