#!/usr/bin/env perl
# SPDX-License-Identifier: GPL-3.0-or-later
#
# LC vocabulary extractor: reads the pinned bulk downloads of id.loc.gov
# (reference/loc-vocabularies/*.skosrdf.nt.gz) and writes
# data/loc/vocabularies.json, the concepts that `cargo xtask codegen` turns into
# Rust (docs/specs/catalogo/bibframe-completo.md, items 11 and 12): the vocabularies
# of id.loc.gov/vocabulary and the genre/form terms of the LCGFT
# (id.loc.gov/authorities/genreForms), which bf:genreForm names by IRI.
#
# Usage (from the repository root):
#   perl tools/bibframe-extractor/vocabularies.pl           # write
#   perl tools/bibframe-extractor/vocabularies.pl --check   # compare, for CI
#
# The files are N-Triples, one triple per line; lines it cannot read abort the run.

use strict;
use warnings;
use utf8;

use Digest::SHA qw(sha256_hex);
use IO::Uncompress::Gunzip qw(gunzip $GunzipError);
use JSON::PP   ();
use File::Path qw(make_path);

use constant OUT_PATH  => 'data/loc/vocabularies.json';
use constant NAMESPACE => 'http://id.loc.gov/vocabulary/';
use constant SKOS      => 'http://www.w3.org/2004/02/skos/core#';
use constant CHANGESET => 'http://purl.org/vocab/changeset/schema#';
use constant GENRE_FORMS => 'http://id.loc.gov/authorities/genreForms';

# The bulk files of https://id.loc.gov/download/, as fetched.
my @SOURCES = qw(cataloging languages countries);

my $check = grep { $_ eq '--check' } @ARGV;

my ( @sources, %concepts, $genre_forms );
for my $name (@SOURCES) {
    my $path = "reference/loc-vocabularies/$name.skosrdf.nt.gz";
    my $gz   = slurp($path);
    gunzip( \$gz => \my $bytes ) or die "$path: $GunzipError\n";
    my $text = $bytes;
    utf8::decode($text) or die "$path is not UTF-8\n";
    push @sources, {
        file   => $path,
        url    => "https://id.loc.gov/download/vocabulary/$name.skosrdf.nt.gz",
        sha256 => sha256_hex($gz),
    };
    read_triples( $path, $text, \%concepts );
}

# The LCGFT is an authority file, not a vocabulary: its terms stay apart from the
# schemes, so that no property offers them as a closed list.
my %genre_concepts;
{
    my $path = 'reference/loc-vocabularies/genreForms.skosrdf.nt.gz';
    my $gz   = slurp($path);
    gunzip( \$gz => \my $bytes ) or die "$path: $GunzipError\n";
    my $text = $bytes;
    utf8::decode($text) or die "$path is not UTF-8\n";
    read_triples( $path, $text, \%genre_concepts );
    my @terms;
    for my $iri ( sort keys %genre_concepts ) {
        my $concept = $genre_concepts{$iri};
        next unless $concept->{concept} && @{ $concept->{in_scheme} };
        grep { $_ eq GENRE_FORMS } @{ $concept->{in_scheme} } or die "$iri is not an LCGFT term\n";
        $iri =~ m{^\Q@{[GENRE_FORMS]}\E/(gf[0-9]+)$} or die "$iri is not an LCGFT identifier\n";
        push @terms, {
            code       => $1,
            label      => label( $iri, $concept ),
            deprecated => $concept->{deprecated} ? JSON::PP::true : JSON::PP::false,
        };
    }
    $genre_forms = {
        file     => $path,
        url      => 'https://id.loc.gov/download/authorities/genreForms.skosrdf.nt.gz',
        sha256   => sha256_hex($gz),
        iri      => GENRE_FORMS,
        concepts => \@terms,
    };
}

my %schemes;
for my $iri ( sort keys %concepts ) {
    my $concept = $concepts{$iri};
    next unless $concept->{concept};
    # A concept of another vocabulary that a record points to (countries point to
    # geographic areas) is typed but has no scheme in this file: it is not ours to list.
    my @in_scheme = @{ $concept->{in_scheme} };
    next unless @in_scheme;
    @in_scheme == 1 or die "$iri is in @{[scalar @in_scheme]} schemes\n";
    my ($scheme) = @in_scheme;
    my $namespace = NAMESPACE;
    $scheme =~ m{^\Q$namespace\E([A-Za-z0-9-]+)$} or die "$iri: scheme $scheme is not an LC vocabulary\n";
    my $name = $1;
    $iri =~ m{^\Q$scheme\E/([^/]+)$} or die "$iri is not under its scheme $scheme\n";
    push @{ $schemes{$name} }, {
        code       => $1,
        label      => label( $iri, $concept ),
        deprecated => $concept->{deprecated} ? JSON::PP::true : JSON::PP::false,
    };
}

my $data = {
    genre_forms => $genre_forms,
    sources => \@sources,
    schemes => [
        map { { name => $_, iri => NAMESPACE . $_, concepts => [ sort { $a->{code} cmp $b->{code} } @{ $schemes{$_} } ] } }
        sort keys %schemes
    ],
};
my $json = JSON::PP->new->canonical->pretty->indent_length(2)->utf8->encode($data);

if ($check) {
    my $current = -e OUT_PATH ? slurp(OUT_PATH) : '';
    if ( $current ne $json ) {
        warn "stale: @{[OUT_PATH]} (run perl tools/bibframe-extractor/vocabularies.pl)\n";
        exit 1;
    }
    exit 0;
}
make_path('data/loc');
spew( OUT_PATH, $json );
my $count = 0;
$count += @{ $_->{concepts} } for @{ $data->{schemes} };
print "wrote @{[OUT_PATH]}: @{[scalar keys %schemes]} schemes, $count concepts, @{[scalar @{ $genre_forms->{concepts} }]} LCGFT terms\n";

# Collects, per subject, what the extractor needs: whether it is a skos:Concept,
# its scheme, its labels by language, and whether a change set deprecates it.
sub read_triples {
    my ( $path, $text, $concepts ) = @_;
    my ( %reason, %subject_of );
    my $line_number = 0;
    for my $line ( split /\n/, $text ) {
        $line_number++;
        next if $line =~ /^\s*(?:#.*)?$/;
        $line =~ m{^(<[^>]*>|_:\w+) <([^>]*)> (.+) \.\s*$}
            or die "$path:$line_number: not a triple\n";
        my ( $subject, $predicate, $object ) = ( $1, $2, $3 );
        if ( $subject =~ /^_:/ ) {
            $reason{$subject} = literal( $path, $line_number, $object )
                if $predicate eq CHANGESET . 'changeReason';
            $subject_of{$subject} = iri( $path, $line_number, $object )
                if $predicate eq CHANGESET . 'subjectOfChange';
            next;
        }
        my $iri = $subject =~ s/^<|>$//gr;
        my $concept = $concepts->{$iri} //= { in_scheme => [], labels => {} };
        if ( $predicate eq 'http://www.w3.org/1999/02/22-rdf-syntax-ns#type' ) {
            $concept->{concept} = 1 if $object eq '<' . SKOS . 'Concept>';
        }
        elsif ( $predicate eq SKOS . 'inScheme' ) {
            push @{ $concept->{in_scheme} }, iri( $path, $line_number, $object );
        }
        elsif ( $predicate eq SKOS . 'prefLabel' ) {
            $object =~ /\@([a-zA-Z-]+)$/;
            my $language = $1 // '';
            $concept->{labels}{$language} = literal( $path, $line_number, $object );
        }
    }
    for my $node ( keys %reason ) {
        next unless $reason{$node} eq 'deprecated' && defined $subject_of{$node};
        $concepts->{ $subject_of{$node} }{deprecated} = 1;
    }
}

# The English label, or the one without a language tag.
sub label {
    my ( $iri, $concept ) = @_;
    my $labels = $concept->{labels};
    my $label  = $labels->{en} // $labels->{''};
    defined $label or die "$iri has no English label\n";
    return $label;
}

sub iri {
    my ( $path, $line_number, $object ) = @_;
    $object =~ /^<([^>]*)>$/ or die "$path:$line_number: expected an IRI\n";
    return $1;
}

# The value of an N-Triples literal, with its escapes undone.
sub literal {
    my ( $path, $line_number, $object ) = @_;
    $object =~ /^"((?:[^"\\]|\\.)*)"(?:\@[a-zA-Z-]+|\^\^<[^>]*>)?$/
        or die "$path:$line_number: expected a literal\n";
    my $value = $1;
    my %escape = ( t => "\t", n => "\n", r => "\r", '"' => '"', "'" => "'", '\\' => '\\', b => "\b", f => "\f" );
    $value =~ s{\\(?:u([0-9A-Fa-f]{4})|U([0-9A-Fa-f]{8})|(.))}{
        defined $1 ? chr hex $1 : defined $2 ? chr hex $2 : ( $escape{$3} // die "$path:$line_number: bad escape \\$3\n" )
    }ge;
    return $value;
}

sub slurp {
    my ($path) = @_;
    open my $handle, '<:raw', $path or die "cannot read $path: $!\n";
    local $/;
    return <$handle>;
}

sub spew {
    my ( $path, $text ) = @_;
    open my $handle, '>:raw', $path or die "cannot write $path: $!\n";
    print {$handle} $text;
    close $handle or die "cannot write $path: $!\n";
}
