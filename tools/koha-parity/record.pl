#!/usr/bin/perl
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Records Koha's own answers on a copy of a library, for `elzevir parity-koha`
# (docs/specs/migracao/koha-parity.md). Read only: it calls Koha's functions and writes
# nothing to the database.
#
#     sudo koha-shell INSTANCE -c 'perl record.pl [--now "YYYY-MM-DD HH:MM"] [--every N]' > answers.json
#
# The output holds Koha's numbers, dates and amounts only: no name, card number or barcode.

use Modern::Perl;
use Getopt::Long qw( GetOptions );
use JSON         qw( to_json );

use C4::Context;
use C4::Circulation qw( CalcDateDue );
use C4::Overdues    qw( Getoverdues CalcFine );
use Koha::Checkouts;
use Koha::DateUtils qw( dt_from_string );
use Koha::Patrons;

my $now_text;
my $every = 1;
GetOptions( 'now=s' => \$now_text, 'every=i' => \$every ) or die "usage: record.pl [--now 'YYYY-MM-DD HH:MM'] [--every N]\n";
die "--every must be at least 1\n" if $every < 1;

my $clock = dt_from_string()->truncate( to => 'minute' );
my $now   = defined $now_text ? dt_from_string( $now_text, 'iso' ) : $clock;
# Getoverdues asks the database for loans due before its own NOW(), so a later moment would
# miss the loans due in between.
die "--now must not be later than the current time\n" if DateTime->compare( $now, $clock ) > 0;

my $format = sub { $_[0]->strftime('%Y-%m-%d %H:%M') };

# The due date each current loan would get if it were made again at its issuedate.
my @due_dates;
my $position  = 0;
my $checkouts = Koha::Checkouts->search( {}, { order_by => 'issue_id' } );
while ( my $checkout = $checkouts->next ) {
    next if $position++ % $every;
    my $item   = $checkout->item;
    my $patron = $checkout->patron;
    next unless $item && $patron && $checkout->issuedate;
    my $branch = C4::Circulation::_GetCircControlBranch( $item, $patron );
    my $due    = CalcDateDue( dt_from_string( $checkout->issuedate ), $item->effective_itemtype, $branch, $patron );
    push @due_dates, {
        issue_id   => $checkout->issue_id + 0,
        itemnumber => $item->itemnumber + 0,
        due        => $format->($due),
    };
}

# The fine of each overdue loan at `now`, with the branch `misc/cronjobs/fines.pl` uses.
my $control     = C4::Context->preference('CircControl');
my $branch_type = C4::Context->preference('HomeOrHoldingBranch') || 'homebranch';
my @fines;
$position = 0;
for my $overdue ( sort { $a->{issue_id} <=> $b->{issue_id} } @{ Getoverdues() } ) {
    next if $position++ % $every;
    next if $overdue->{itemlost};
    next unless defined $overdue->{borrowernumber};
    my $patron = Koha::Patrons->find( $overdue->{borrowernumber} ) or next;
    my $branch =
          $control eq 'ItemHomeLibrary' ? $overdue->{$branch_type}
        : $control eq 'PatronLibrary'   ? $patron->branchcode
        :                                 $overdue->{branchcode};
    my $due = dt_from_string( $overdue->{date_due} );
    next if DateTime->compare( $due, $now ) >= 0;
    my ($amount) = CalcFine( $overdue, $patron->categorycode, $branch, $due, $now->clone );
    push @fines, {
        issue_id     => $overdue->{issue_id} + 0,
        itemnumber   => $overdue->{itemnumber} + 0,
        amount_cents => 0 + sprintf( '%.0f', ( $amount // 0 ) * 100 ),
    };
}

print to_json(
    {
        koha_version => C4::Context->preference('Version'),
        now          => $format->($now),
        due_dates    => \@due_dates,
        fines        => \@fines,
    },
    { canonical => 1, pretty => 1 }
);
