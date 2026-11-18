# Elzevir interface texts, English (primary language).
# Keys: <area>-<page>-<element>. Every key used in a template must exist here
# (checked by the `every_template_key_exists` test).

## Shared
app-name = Elzevir
nav-catalog = Catalog
nav-staff = Staff

## OPAC
opac-home-title = Library catalog
opac-home-search-label = Search the catalog
opac-home-search-placeholder = Title, author or subject
opac-home-search-button = Search

## Staff login
staff-login-title = Staff sign in
staff-login-user = Username
staff-login-password = Password
staff-login-submit = Sign in
staff-login-invalid = Invalid username or password.
staff-login-not-staff = This account cannot use the staff interface.
staff-login-unavailable = Sign in is temporarily unavailable. Try again shortly.

## Staff home
staff-home-title = Staff interface
staff-home-signed-in-as = Signed in as
staff-home-administration = Administration
staff-home-logout = Sign out

## OPAC results
opac-results-title = Search results
opac-results-count = { $count ->
    [one] 1 result
   *[other] { $count } results
}
opac-results-none = Nothing found for “{ $query }”.
opac-results-pages = Result pages
opac-results-previous = Previous
opac-results-next = Next
opac-availability-available = { $available } of { $items } available
opac-availability-none = No copies available
opac-availability-no-items = No copies

## OPAC record
opac-record-contributors = Authors
opac-record-material = Type
opac-record-publication = Publication
opac-record-classification = Classification
opac-record-subjects = Subjects
opac-record-identifiers = Identifiers
opac-record-languages = Languages
opac-record-summary = Summary
opac-record-audience = Audience
opac-record-genres = Genre and form
opac-record-other-titles = Other titles
opac-record-contents = Contents
opac-record-notes = Notes
opac-record-items = Copies
opac-record-no-items = This record has no copies.
opac-record-item-branch = Library
opac-record-item-location = Shelf
opac-record-item-call-number = Call number
opac-record-item-status = Status
opac-not-found-title = Record not found
opac-not-found-back = Back to the catalog

## Item status
item-status-available = Available
item-status-not-for-loan = Not for loan
item-status-damaged = Damaged
item-status-lost = Lost
item-status-withdrawn = Withdrawn
item-status-restricted = Restricted

## Content and carrier, as their icons say them (bibframe-completo.md, item 15)
content-text = Text
content-notated-music = Notated music
content-audio = Audio
content-moving-image = Moving image
content-still-image = Still image
content-serial = Serial
content-tactile = Tactile
content-cartography = Cartography
content-object = Object
content-multimedia = Multimedia
content-mixed = Mixed material
content-dataset = Dataset
carrier-print = Print
carrier-online = Online
carrier-disc = Disc

# Staff catalog listing
staff-home-catalog = Catalog
staff-catalog-title = Catalog
staff-catalog-search-label = Search the catalog
staff-catalog-search-placeholder = Title, author, subject, ISBN, call number, barcode
staff-catalog-search-button = Search
staff-catalog-next = More records
staff-catalog-empty = Nothing to show.
facet-strip-label = Narrow the listing
facet-families = { $count ->
    [one] One family
   *[other] { $count } families
}
facet-remove = Remove this filter
facet-clear = Clear filters
list-label = Work lists
list-all = Whole catalog
list-recent = My recent
list-imported-today = Imported today
list-empty = Nothing in this list.
selection-select = Select this family
selection-label = Selected families
selection-count = Selected:
selection-full = The limit is 500 families.
selection-export-marc21 = Export MARC21
selection-export-marcxml = Export MARCXML
selection-clear = Clear selection
facet-branch = Branch
facet-content = Content
facet-carrier = Carrier
facet-genre = Genre
facet-audience = Audience
facet-language = Language
facet-subject = Subject
facet-year = Year
staff-catalog-count = { $count ->
    [one] One record found
   *[other] { $count } records found
}
staff-catalog-more-items = { $count ->
    [one] +1 item — see all
   *[other] +{ $count } items — see all
}
shelf-signal-available = Available
shelf-signal-not-for-loan = Not for loan
shelf-signal-out-of-circulation = Out of circulation
staff-cord-work = work
staff-cord-edition = edition
staff-cord-items = items
staff-column-hub = Hub
staff-catalog-columns = Columns shown
staff-row-shape = { $works ->
    [one] One work
   *[other] { $works } works
}, { $instances ->
    [one] one instance
   *[other] { $instances } instances
}
staff-row-available = { $available } of { $items } available
staff-catalog-focus-label = Family in focus
staff-export = Export
staff-column-authorities = Authorities
staff-column-work = Work
staff-column-instance = Instance
staff-column-items = Items
staff-badge-hub = Hub
staff-badge-agent = Agent
staff-badge-subject = Subject
staff-field-authorized-name = Authorized name
authority-reach = Saving changes { $works ->
    [one] one Work
   *[other] { $works } Works
} and { $instances ->
    [one] one record
   *[other] { $instances } records
}.
authority-identifiers = Also identified in
link-authority = Authority
link-authority-label = Link an authority, stored or new, to the Work in focus
link-heading = Link an authority
link-authority-field = Authority and its role
link-column-class = Class
link-column-role = Role in the Work
link-column-relator = Relator code
link-submit = Link
link-role-contribution = contributor
link-role-subject = subject
link-role-place = place of publication
link-instance = Instance of a place of publication
link-error-not-an-agent = Only a person, family, organization or meeting can be a contributor.
link-error-not-a-place = Only a place can be a place of publication.
link-error-no-instance = The Work has no Instance to give a place of publication.
link-error-already-linked = The Work already names this authority in this role.
staff-cord-written-by = written by
staff-cord-about = about
staff-badge-work = Work
staff-badge-instance = Instance
staff-field-uniform-title = Uniform title
staff-field-work-title = Work title
staff-field-agents = Agents
staff-field-classification = Classification
staff-field-subjects = Subjects
staff-field-title = Title
staff-field-imprint = Imprint
staff-field-isbn = ISBN
staff-field-material = Material

## Every field of a focused block
staff-fields-work = Work
staff-fields-instance = Instance
staff-fields-items = Items
staff-fields-preferred-title = Preferred title
staff-fields-material = Content
staff-fields-carrier = Carrier
staff-fields-work-classes = Classes of the Work
staff-fields-instance-classes = Classes of the Instance
staff-fields-classification = Classification
staff-fields-languages = Languages
staff-fields-contributions = Contributions
staff-fields-primary = main entry
staff-fields-subjects = Subjects
staff-fields-summary = Summary
staff-fields-audience = Intended audience
staff-fields-relations = Related works
staff-fields-title = Title
staff-fields-edition = Edition
staff-fields-place = Place of publication
staff-fields-publisher = Publisher
staff-fields-date = Date of publication
staff-fields-extent = Extent
staff-fields-series = Series statement
staff-fields-opac-hidden = Hidden from the OPAC
staff-fields-opac-hidden-yes = Yes
staff-fields-identifiers = Identifiers
staff-fields-note-general = General note
staff-fields-note-contents = Contents
staff-fields-note-local = Local note
staff-fields-item-status = Status
staff-fields-barcode = Barcode
staff-fields-inventory-number = Inventory number
staff-fields-call-number = Call number
staff-fields-branch = Branch
staff-fields-location = Location
staff-fields-collection = Collection
staff-fields-policy = Circulation policy
staff-fields-copy = Copy
staff-fields-acquired = Acquired
staff-agent-person = person
staff-agent-organization = organization
staff-agent-meeting = meeting
staff-agent-family = family
staff-subject-topic = topic
staff-subject-place = place
staff-subject-person = person
staff-subject-organization = organization
staff-subject-meeting = meeting
staff-subject-family = family
staff-subject-title = title
staff-subject-genre-form = genre/form
staff-subject-temporal = period
staff-identifier-isbn = ISBN
staff-identifier-issn = ISSN
staff-identifier-lccn = LCCN
staff-identifier-ean = EAN
staff-identifier-system-control-number = control number
staff-identifier-doi = DOI
staff-identifier-ismn = ISMN
staff-relation-translation-of = translation of
staff-relation-adaptation-of = adaptation of
staff-relation-part-of = part of
staff-relation-supplement-to = supplement to
staff-relation-continuation-of = continuation of
staff-relation-work = Work { $id }

## Editing refusals
edit-error-title-empty = The title cannot be empty.
edit-error-heading-empty = The authorized name cannot be empty.
edit-error-heading-taken = Another agent already has this name; joining the two is a merge.
edit-error-subject-taken = Another subject of this kind and thesaurus already has this heading; joining the two is a merge.
edit-error-subdivisions = Keep as many subdivisions (parts after --) as the subject has; change them in the Work's form.
edit-error-language-code = Use three-letter language codes, such as por or eng.
edit-error-isbn = This ISBN is not valid: check the digits.
edit-isbn-converted = Converted from the ISBN-10 { $isbn10 }.
edit-isbn-ten = ISBN-10: { $isbn10 }
edit-error-issn = This ISSN is not valid: check the digits.
edit-error-class = Choose classes of this entity from the list.

## Editing in the focused block
edit-done = Done
history-heading = History
history-empty = No change recorded yet.
history-unknown-time = before the history
history-system = import or command line
history-created = Created
history-imported = Imported from a file
history-edited = Changed: { $parts }
history-edited-whole = Description replaced
history-items = { $count ->
    [one] One Item saved
   *[other] { $count } Items saved
}
history-other = Other change
edit-done-hint = Ctrl+Enter
edit-saved = Saved
edit-add-row = Add
edit-remove-row = Remove
edit-discard-question = Discard your changes?
edit-discard = Discard
edit-keep = Keep editing
edit-stale = Someone else changed this record. Reload to see their changes before editing.
edit-failed = The edit could not be saved. Try again.
edit-field-notes = Notes
edit-column-name = Name
edit-column-kind = Type
edit-column-role = Role
edit-column-number = Number
edit-column-scheme = Scheme
edit-column-heading = Heading
edit-column-thesaurus = Thesaurus
edit-column-value = Value
edit-column-qualifier = Qualifier
edit-column-text = Text
edit-column-language = Language
edit-column-concept = Term
edit-column-class = Class
edit-column-label = Name
edit-column-iri = IRI
edit-column-details = Its own statements, one per line (property: value)
edit-column-work = Work number
edit-column-kept = Stored value
edit-section-title = Title
edit-section-content = Content
edit-section-contributions = Contributions
edit-section-subjects = Subjects
edit-section-classification = Classification
edit-section-responsibility = Responsibility
edit-section-edition = Edition
edit-section-provision = Publication, production and distribution
edit-section-physical-description = Physical description
edit-section-media-and-carrier = Media and carrier
edit-section-identifiers = Identifiers
edit-section-series = Series
edit-section-electronic-locator = Electronic location
edit-section-location = Location
edit-section-relations = Relations
edit-section-notes = Notes
edit-section-admin-metadata = Administrative metadata
edit-error-relation-itself = A Work cannot relate to itself.
edit-error-statement = These values cannot be stored: check each row.
edit-new-entity = new
edit-inherited = from the previous record
items-error-required = This column cannot be empty.
items-grid-title = Items
items-edit = Edit items
items-add-count = Rows
items-add-inventory = First inventory number
items-add-step = Step
items-add-rows = Add rows
items-save = Save
items-describe = Full description of the item
items-close = Close
items-refused = Some cells were refused; see the marked cells.
items-column-status = Status
items-column-barcode = Barcode
items-column-inventory = Inventory number
items-column-call-number = Call number
items-suggested-call-number = Suggested call number:
items-suggested-entry = table entry
items-column-home-branch = Home branch
items-column-holding-branch = Current branch
items-column-location = Location
items-column-collection = Collection
items-column-policy = Circulation policy
items-column-copy = Copy
items-column-price = Price
items-keys-hint = Arrows move, Enter edits and goes down, F2 edits, Ctrl+D copies the cell above, Esc cancels. A scanned barcode on the last row adds a row.
items-error-price = Write the price as 12,50 or 12.50.
items-error-branch = No branch has this id.
items-error-not-in-branch = The home branch has no such shelving place or collection.
items-error-policy = No circulation policy has this id.
items-error-barcode-taken = Another item already has this barcode.
items-error-unknown-item = This item is not one of this edition's items.
new-family = New family
new-family-hint = N
new-instance = Instance
new-instance-label = New Instance of this Work
new-family-heading = New family
new-instance-heading = New Instance
new-instance-copy = Like this
new-instance-copy-label = New Instance like this one, without its identifiers and Items
new-instance-copy-heading = New Instance like this one
new-work-part = Work
new-instance-part = Instance
new-create = Create
new-cancel = Cancel

## Code tables (docs/specs/catalogo/tabelas-de-codigos.md)
registries-title = Branches and policies
registries-intro = Branches, their shelving places and collections, and circulation policies are the library's own lists. Items point at them; a name can change at any time.
registries-branches = Branches
registries-sublocations = Shelving places
registries-collections = Collections
registries-policies = Circulation policies
registries-policies-hint = Loan periods, renewals and fines come with circulation.
registries-name = Name
registries-new = New name
registries-new-branch = New branch
registries-add = Add
registries-save = Save
registries-delete = Delete
registries-address = Address
registries-phone = Phone
registries-email = E-mail
registries-url = Web page
registries-saved = Saved.
registries-error-name = Give a name of at most 256 characters; contact fields take at most 512.
registries-error-taken = Another row of this list already has this name.
registries-error-in-use = It is in use, so it stays. Move what uses it first.
registries-error-unknown = That row no longer exists.

# Prepared import
import-title = Import catalog records
import-file = Source file
import-limit = ISO 2709, MARCXML or MarcEdit (.mrk), up to 512 MiB and 100,000 records.
import-upload = Prepare batch
import-progress = Upload progress
import-drafts = Your prepared batches
import-drafts-back = Back to your batches
import-revision = Revision
import-recipe = Edit recipe
import-recipe-help = Use record numbers from the source. Omit record to apply an edit throughout the batch. The recipe also accepts exclusions, matching choices and explicit acceptance of missing barcodes.
import-recipe-example = Example recipe for editing a title
import-save = Save choices
import-preview = Review preview
import-details = Details
import-work = Work
import-instance = Instance
import-items = Items
import-record = Record
import-source = Original MARC
import-edited = Edited MARC
import-candidate = Possible match with instance
import-record-pages = Record pages
import-diagnostic-pages = Diagnostic pages
import-next = Next page
import-previous = Previous page
import-diagnostics = Problems to review
import-confirm = Confirm import
import-cancel = Cancel batch
import-cancel-copy = Canceling removes the original file and saved edits.
import-confirm-cancel = Remove this batch
import-acknowledge = I checked the catalog after the interrupted confirmation.
import-result = Import result
import-removed = The original file and saved edits were removed.
import-download = Download report (JSON)
import-more-refusals = Additional refusal locations are in the downloaded report.
import-preparing = Preparing
import-review = Awaiting review
import-ready = Ready for confirmation
import-interrupted = Confirmation interrupted
import-closed = Cleanup pending
import-iso = ISO 2709
import-xml = MARCXML
import-mrk = MarcEdit
import-unknown = Format pending
import-evidence-iso = The file has an ISO 2709 leader.
import-evidence-xml = The XML root uses the MARC21 namespace.
import-evidence-mrk = The file starts with a MarcEdit leader line.
import-evidence-empty = The source contains no records.
import-evidence-unknown = The file format could not be determined.
import-summary = { $read } source records · { $excluded } excluded · { $changed } edited · { $items } items to submit · { $matches } possible matches · { $replacements } possible replacements
import-diagnostic-record = Record { $record }, byte { $byte }
import-diagnostic-item = Record { $record }, byte { $byte }, item { $item }
import-stale = This revision cannot be used. Open your batch to review its current state.
import-invalid = The source or choices require review.
import-upload-invalid = The upload must contain one source file after the form token.
import-forbidden = This form token is invalid. Open the import page again.
import-permission = Cataloguing permission is required.
import-missing = This batch or report is unavailable.
import-unavailable = The import service is unavailable. Try again.
import-incomplete = Confirmation or cleanup was interrupted. Check the catalog before canceling the batch.
import-read = Source records
import-prepared = Prepared
import-excluded = Excluded
import-stored = Stored
import-matched = Matched
import-replaced = Replaced
import-skipped = Skipped
import-failed = Records refused
import-items-stored = Items added
import-items-replaced = Items replaced
import-items-refused = Items refused
import-format = Source format
import-format-auto = Detect from the file
import-recent-results = Your recent results
import-problem-record = The source record could not be prepared.
import-problem-description = The bibliographic description did not pass validation.
import-problem-branch = Unknown branch.
import-problem-item-type = Unknown item type.
import-problem-registry = An item registry check failed.
import-problem-missing-barcode = Missing barcode.
import-problem-repeated-barcode = This barcode appears more than once in the batch.
import-problem-used-barcode = This barcode is already in the catalog.
import-problem-match = Possible match with record { $record } in this batch.
import-replace-items-note = The selected matching action can replace catalog items with the same barcode.
import-table = Import CSV or TSV
import-table-limit = CSV or TSV in UTF-8 or Latin-1, up to 512 MiB.
import-evidence-table = The spreadsheet was read with its selected separator.
import-model-csv = Download the BIBFRAME CSV model
import-model-tsv = Download the BIBFRAME TSV model
import-table-recipe = Save a versioned TOML mapping or keep the generated model header. Save a new revision to change the separator, encoding, grouping or matching.
