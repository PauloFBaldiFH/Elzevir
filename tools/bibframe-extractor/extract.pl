#!/usr/bin/env perl
# SPDX-License-Identifier: GPL-3.0-or-later
#
# BIBFRAME extractor: reads the pinned Library of Congress ontology
# (reference/bibframe/bibframe.rdf) and writes data/loc/bibframe.json, the list
# of classes and properties that `cargo xtask codegen` turns into Rust.
#
# Usage (from the repository root):
#   perl tools/bibframe-extractor/extract.pl           # write
#   perl tools/bibframe-extractor/extract.pl --check   # compare, for CI
#
# The parser is a restricted reader for the layout of that one file (core Perl
# has no XML parser): any element it does not expect aborts the run.

use strict;
use warnings;
use utf8;

use Digest::SHA qw(sha256_hex);
use JSON::PP    ();
use File::Path  qw(make_path);

use constant SOURCE_URL  => 'https://id.loc.gov/ontologies/bibframe.rdf';
use constant SOURCE_PATH => 'reference/bibframe/bibframe.rdf';
use constant OUT_PATH    => 'data/loc/bibframe.json';
use constant NAMESPACE   => 'http://id.loc.gov/ontologies/bibframe/';

my $check = grep { $_ eq '--check' } @ARGV;

my $bytes = slurp(SOURCE_PATH);
my $xml   = $bytes;
utf8::decode($xml) or die "@{[SOURCE_PATH]} is not UTF-8\n";
$xml =~ /<!--|<!\[CDATA\[|<!DOCTYPE/ and die "unexpected XML construct in @{[SOURCE_PATH]}\n";

my ( @classes, @properties, $ontology );
while ( $xml =~ m{<owl:(Ontology|Class|ObjectProperty|DatatypeProperty|SymmetricProperty)\s+rdf:about="([^"]*)"\s*>(.*?)</owl:\1>}gs ) {
    my ( $kind, $about, $body ) = ( $1, $2, $3 );
    my $fields = fields( $about, $body );
    if ( $kind eq 'Ontology' ) {
        $ontology = {
            url         => SOURCE_URL,
            sha256      => sha256_hex($bytes),
            version     => one( $about, $fields, 'owl:versionInfo' ),
            version_iri => one( $about, $fields, 'owl:versionIRI' ),
            issued      => one( $about, $fields, 'dcterms:issued' ),
        };
        next;
    }
    my $entry = {
        name       => local_name($about),
        label      => one( $about, $fields, 'rdfs:label' ),
        definition => one( $about, $fields, 'skos:definition' ),
    };
    if ( $kind eq 'Class' ) {
        $entry->{sub_class_of} = names( $fields, 'rdfs:subClassOf' );
        push @classes, $entry;
        next;
    }
    $entry->{kind} = lc( $kind =~ s/Property$//r );
    $entry->{domain}          = names( $fields, 'rdfs:domain' );
    $entry->{range}           = names( $fields, 'rdfs:range' );
    $entry->{sub_property_of} = names( $fields, 'rdfs:subPropertyOf' );
    $entry->{inverse_of}      = names( $fields, 'owl:inverseOf' );
    $entry->{used_with}       = suggested( $about, $fields, 'use' );
    $entry->{suggested_value} = suggested( $about, $fields, 'value' );
    push @properties, $entry;
}
$ontology or die "no owl:Ontology in @{[SOURCE_PATH]}\n";

# Every top-level element must have been read: a new kind of element is a
# reason to stop, not to skip.
my $top_level = () = $xml =~ m{^  <[a-z]+:[A-Za-z]+[ >]}gm;
$top_level == 1 + @classes + @properties
    or die "read @{[1 + @classes + @properties]} of $top_level top-level elements\n";

my $data = {
    source     => $ontology,
    classes    => [ sort { $a->{name} cmp $b->{name} } @classes ],
    properties => [ sort { $a->{name} cmp $b->{name} } @properties ],
};
my $text = JSON::PP->new->canonical->pretty->indent_length(2)->utf8->encode($data);

if ($check) {
    my $current = -e OUT_PATH ? slurp(OUT_PATH) : '';
    if ( $current ne $text ) {
        warn "stale: @{[OUT_PATH]} (run perl tools/bibframe-extractor/extract.pl)\n";
        exit 1;
    }
    exit 0;
}
make_path('data/loc');
spew( OUT_PATH, $text );
print "wrote @{[OUT_PATH]}: @{[scalar @classes]} classes, @{[scalar @properties]} properties\n";

# The child elements of one description: name => list of values, where a value
# is the rdf:resource or the text with whitespace collapsed.
sub fields {
    my ( $about, $body ) = @_;
    my %fields;
    my $rest = $body;
    while ( $rest =~ s{\A\s*<([a-z-]+:[A-Za-z]+)((?:\s+[a-z:]+="[^"]*")*)\s*(?:/>|>(.*?)</\1>)}{}s ) {
        my ( $name, $attributes, $text ) = ( $1, $2, $3 );
        my $value;
        if ( defined $text ) {
            $text =~ /</ and die "$about: nested element in $name\n";
            $value = unescape($text) =~ s/\s+/ /gr =~ s/^ | $//gr;
        }
        elsif ( $attributes =~ /\brdfs?:resource="([^"]*)"/ ) {
            $value = unescape($1);
        }
        else {
            die "$about: $name has neither text nor a resource\n";
        }
        push @{ $fields{$name} }, $value;
    }
    $rest =~ /\S/ and die "$about: cannot read: " . substr( $rest, 0, 80 ) . "\n";
    my %known = map { $_ => 1 } qw(
        rdfs:label skos:definition rdfs:subClassOf rdfs:domain rdfs:range
        rdfs:subPropertyOf owl:inverseOf rdfs:comment dcterms:modified
        skos:editorialNote owl:versionInfo owl:versionIRI owl:priorVersion
        dcterms:issued dcterms:description dcterms:creator dcterms:publisher
        dcterms:rights cc:license
    );
    for my $name ( sort keys %fields ) {
        $known{$name} or die "$about: unexpected element $name\n";
    }
    return \%fields;
}

sub one {
    my ( $about, $fields, $name ) = @_;
    my @values = @{ $fields->{$name} // [] };
    @values == 1 or die "$about: expected one $name, found @{[scalar @values]}\n";
    return $values[0];
}

# Resources of a field, with the BIBFRAME namespace removed, in file order.
sub names {
    my ( $fields, $name ) = @_;
    return [ map { local_name($_) } @{ $fields->{$name} // [] } ];
}

sub local_name {
    my ($iri) = @_;
    my $namespace = NAMESPACE;
    return $iri =~ s/^\Q$namespace\E//r;
}

# The ontology states where a property without a domain is used, and what its
# values are, in comments: "Suggested use - With Work or Instance",
# "Suggested value - bf:Place or madsrdf:Geographic".
sub suggested {
    my ( $about, $fields, $what ) = @_;
    my @found;
    for my $comment ( @{ $fields->{'rdfs:comment'} // [] } ) {
        $comment =~ /^Suggested (use|value) - (?:With )?(.+)$/
            or die "$about: unexpected comment: $comment\n";
        next if $1 ne $what;
        push @found, map { s/^bf://r } split /, | or /, $2;
    }
    return \@found;
}

sub unescape {
    my ($text) = @_;
    $text =~ /&(?!(?:amp|lt|gt|quot|apos);)/ and die "unexpected entity in: $text\n";
    my %entity = ( amp => '&', lt => '<', gt => '>', quot => '"', apos => "'" );
    return $text =~ s/&(amp|lt|gt|quot|apos);/$entity{$1}/gr;
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
