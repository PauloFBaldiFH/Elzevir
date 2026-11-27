#!/usr/bin/perl
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Does a staged desk day on a COPY of a library, with Koha's own functions, for
# `elzevir desk-day` (docs/specs/migracao/desk-day.md). It WRITES to the database: back the
# copy up before and restore it after.
#
#     sudo koha-shell INSTANCE -c 'perl desk_day.pl --on-a-copy /tmp/plan.json' > day.json
#
# The output holds Koha's numbers, keys, dates and amounts only: no name, card number or
# barcode.

use Modern::Perl;
use Getopt::Long qw( GetOptions );
use JSON         qw( from_json to_json );

use C4::Context;
use C4::Circulation qw( AddIssue AddRenewal AddReturn CanBookBeIssued CanBookBeRenewed );
use C4::Reserves    qw( AddReserve CalculatePriority CanBookBeReserved CanItemBeReserved );
use Koha::DateUtils qw( dt_from_string );
use Koha::Holds;
use Koha::Items;
use Koha::Patrons;

my $on_a_copy;
GetOptions( 'on-a-copy' => \$on_a_copy ) or die "usage: desk_day.pl --on-a-copy PLAN.json\n";
die "desk_day.pl writes to the database: run it on a copy, with --on-a-copy\n" unless $on_a_copy;
my $plan_path = shift @ARGV or die "usage: desk_day.pl --on-a-copy PLAN.json\n";

open my $plan_file, '<', $plan_path or die "$plan_path: $!\n";
my $plan = from_json( do { local $/; <$plan_file> } );
die "$plan_path: the plan is a list of steps\n" unless ref $plan eq 'ARRAY';
die "$plan_path: more than 10000 steps\n" if @$plan > 10_000;

my $format = sub { $_[0]->strftime('%Y-%m-%d %H:%M') };

# The desk is the branch where the Item is, or the patron's branch (item 3).
sub desk {
    my ($branch) = @_;
    # No staff member stands behind the steps: `manager_id` stays empty.
    C4::Context->set_userenv( undef, undef, undef, 'Parity', 'Parity', $branch, $branch, 0 );
}

# The patron's outstanding debits, in cents (item 5).
sub balance {
    my ($patron) = @_;
    return undef unless $patron;
    my $owed = $patron->account->outstanding_debits->total_outstanding // 0;
    return 0 + sprintf( '%.0f', $owed * 100 );
}

# The uppercase keys of a CanBookBeIssued hash, sorted: the others carry values.
sub keys_of {
    my ($hash) = @_;
    return join ',', sort grep { /^[A-Z_]+$/ } keys %{ $hash // {} };
}

sub checkout {
    my ( $step, $patron, $item ) = @_;
    desk( $item->holdingbranch );
    my ( $impossible, $confirm ) = CanBookBeIssued( $patron, $item->barcode );
    my $impossible_keys = keys_of($impossible);
    return "impossible $impossible_keys" if $impossible_keys;
    my $confirm_keys = keys_of($confirm);
    return "confirm $confirm_keys" if $confirm_keys && !$step->{confirm};
    my $checkout = AddIssue( $patron, $item->barcode );
    return 'unknown' unless $checkout;
    return 'ok due ' . $format->( dt_from_string( $checkout->date_due ) );
}

sub checkin {
    my ($item) = @_;
    desk( $item->holdingbranch );
    my ( $returned, $messages ) = AddReturn( $item->barcode, $item->holdingbranch );
    my $state = $returned ? 'ok' : $messages->{NotIssued} ? 'not_on_loan' : 'refused';
    my $found = $messages->{ResFound};
    my $hold  = $found ? "$found->{ResFound} borrowernumber $found->{borrowernumber}" : 'none';
    return "$state; $hold";
}

sub renew {
    my ( $patron, $item ) = @_;
    my $checkout = $item->checkout or return 'no_checkout';
    desk( $item->holdingbranch );
    my ( $renewable, $reason ) = CanBookBeRenewed( $patron, $checkout );
    return $reason // 'unknown' unless $renewable;
    my $due = AddRenewal(
        {
            borrowernumber => $patron->borrowernumber,
            itemnumber     => $item->itemnumber,
            branch         => $item->holdingbranch,
        }
    );
    return 'ok due ' . $format->( dt_from_string($due) );
}

sub hold {
    my ( $step, $patron, $item ) = @_;
    my $pickup = $patron->branchcode;
    desk($pickup);
    my $level = $step->{level} // 'record';
    my $can =
        $level eq 'item'
        ? CanItemBeReserved( $patron, $item, $pickup )
        : CanBookBeReserved( $patron->borrowernumber, $item->biblionumber, $pickup );
    return $can->{status} unless $can->{status} eq 'OK';
    my $hold_id = AddReserve(
        {
            branchcode     => $pickup,
            borrowernumber => $patron->borrowernumber,
            biblionumber   => $item->biblionumber,
            itemnumber     => $level eq 'item' ? $item->itemnumber : undef,
            priority       => CalculatePriority( $item->biblionumber ),
        }
    );
    my $placed = $hold_id && Koha::Holds->find($hold_id);
    return $placed ? 'ok priority ' . $placed->priority : 'unknown';
}

sub pay {
    my ( $step, $patron ) = @_;
    desk( $patron->branchcode );
    $patron->account->pay(
        {
            amount     => $step->{amount_cents} / 100,
            type       => 'PAYMENT',
            library_id => $patron->branchcode,
            interface  => 'intranet',
        }
    );
    return 'ok';
}

my @steps;
for my $step (@$plan) {
    my $op     = $step->{op} // '';
    my $item   = defined $step->{itemnumber} ? Koha::Items->find( $step->{itemnumber} ) : undef;
    my $patron = defined $step->{borrowernumber} ? Koha::Patrons->find( $step->{borrowernumber} ) : undef;
    # The patron of a checkin or renewal is the one who has the loan.
    if ( ( $op eq 'checkin' || $op eq 'renew' ) && $item && $item->checkout ) {
        $patron = $item->checkout->patron;
    }
    my $at = $format->( dt_from_string() );
    my $answer;
    if ( $op eq 'pay' ) {
        $answer = $patron ? pay( $step, $patron ) : 'unknown';
    } elsif ( !$item ) {
        $answer = 'unknown';
    } elsif ( $op eq 'checkout' ) {
        $answer = $patron ? checkout( $step, $patron, $item ) : 'unknown';
    } elsif ( $op eq 'checkin' ) {
        $answer = checkin($item);
    } elsif ( $op eq 'renew' ) {
        $answer = $patron ? renew( $patron, $item ) : 'no_checkout';
    } elsif ( $op eq 'hold' ) {
        $answer = $patron ? hold( $step, $patron, $item ) : 'unknown';
    } else {
        die "step " . ( @steps + 1 ) . ": unknown op '$op'\n";
    }
    # The balance is read again, after the step changed it.
    $patron = Koha::Patrons->find( $patron->borrowernumber ) if $patron;
    push @steps, { %$step, at => $at, answer => $answer, balance_cents => balance($patron) };
}

print to_json(
    {
        koha_version => C4::Context->preference('Version'),
        steps        => \@steps,
    },
    { canonical => 1, pretty => 1 }
);
