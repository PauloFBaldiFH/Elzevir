//! CARANGUEJO BIBLIOTECÁRIO — reprodução vetorial fiel, SEM FUNDO.
//!
//! Este programa descreve o personagem usando apenas geometrias SVG geradas em Rust.
//! NÃO incorpora PNG/JPG, imagem de referência, nem usa base64 ou filtros para
//! simular um fundo transparente. O canal alfa é naturalmente transparente.
//!
//! Executar: cargo run --release
//! Saídas: caranguejo_transparente.svg (quadrado)
//!         caranguejo_recortado.svg     (apenas personagem, margem pequena)
//!
//! Sem dependências externas — Rust standard library.

use std::fmt::Write as _;

/// Dimensões da arte base usada para traçar as coordenadas.
const BASE: u32 = 1248;
/// Região centralizada no personagem, preservando uma margem de segurança.
const CROP_X: u32 = 176;
const CROP_Y: u32 = 245;
const CROP_W: u32 = 962;
const CROP_H: u32 = 739;

/// Vetor completo, fundo transparente, enquadramento quadrado de 1248 x 1248.
pub fn svg() -> String {
    compose("0 0 1248 1248", BASE, BASE)
}

/// Vetor recortado em volta do personagem, preservando a proporção original.
pub fn svg_recortado() -> String {
    let view_box = format!("{} {} {} {}", CROP_X, CROP_Y, CROP_W, CROP_H);
    compose(&view_box, CROP_W, CROP_H)
}

fn compose(view_box: &str, width: u32, height: u32) -> String {
    let mut svg = String::with_capacity(46_000);
    writeln!(
        svg,
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="{view_box}" role="img" aria-label="Caranguejo bibliotecário laranja de óculos, blazer, gravata e livro sobre fundo transparente">"##
    ).expect("formatação SVG");

    // Ordem de pintura: de trás para frente. Nenhuma camada desenha fundo.
    definitions(&mut svg);
    legs(&mut svg);
    carapace(&mut svg);
    raised_claw(&mut svg);
    blazer_base(&mut svg);
    shirt_vest_tie(&mut svg);
    blazer_lapels(&mut svg);
    left_book_and_claw(&mut svg);
    face(&mut svg);
    details(&mut svg);
    svg.push_str("</svg>\n");
    svg
}

fn definitions(svg: &mut String) {
    svg.push_str(r##"
<defs>
  <linearGradient id="shell" x1="0" y1="0" x2=".96" y2="1">
    <stop offset="0" stop-color="#fe5900"/>
    <stop offset=".55" stop-color="#f74a00"/>
    <stop offset="1" stop-color="#e94400"/>
  </linearGradient>
  <linearGradient id="leg" x1="0" y1="0" x2="1" y2=".4">
    <stop offset="0" stop-color="#ff5700"/>
    <stop offset="1" stop-color="#c73504"/>
  </linearGradient>
  <linearGradient id="claw" x1="0" y1="0" x2=".8" y2="1">
    <stop offset="0" stop-color="#ff6800"/>
    <stop offset=".5" stop-color="#fc5000"/>
    <stop offset="1" stop-color="#e84405"/>
  </linearGradient>
  <linearGradient id="wool" x1="0" y1="0" x2="1" y2="1">
    <stop offset="0" stop-color="#70503d"/>
    <stop offset=".5" stop-color="#614534"/>
    <stop offset="1" stop-color="#7a5036"/>
  </linearGradient>
  <linearGradient id="vest" x1="0" y1="0" x2="1" y2=".8">
    <stop offset="0" stop-color="#b1a354"/>
    <stop offset=".58" stop-color="#9c8b41"/>
    <stop offset="1" stop-color="#7f7137"/>
  </linearGradient>
  <linearGradient id="bookCover" x1="0" y1="0" x2="1" y2=".85">
    <stop offset="0" stop-color="#285d62"/>
    <stop offset=".48" stop-color="#184c50"/>
    <stop offset="1" stop-color="#123b40"/>
  </linearGradient>
  <linearGradient id="lens" x1="0" y1="0" x2="1" y2="1">
    <stop offset="0" stop-color="#ffbd8d"/>
    <stop offset="1" stop-color="#fa8c5e"/>
  </linearGradient>
  <linearGradient id="whiteShirt" x1="0" y1="0" x2=".7" y2="1">
    <stop offset="0" stop-color="#fffaf0"/>
    <stop offset="1" stop-color="#e2d5b3"/>
  </linearGradient>
  <linearGradient id="tie" x1="0" y1="0" x2="1" y2="1">
    <stop offset="0" stop-color="#87304a"/>
    <stop offset=".5" stop-color="#641b32"/>
    <stop offset="1" stop-color="#7f263a"/>
  </linearGradient>
  <pattern id="herringbone" patternUnits="userSpaceOnUse" width="20" height="20">
    <path d="M-3 1 5 8 12 0 M7 1 15 8 22 0 M-3 11 5 18 12 10 M7 11 15 18 22 10" fill="none" stroke="#b98b5f" stroke-width="1.7" opacity=".25"/>
    <path d="M2 -3 9 4 17 -3 M2 7 9 14 17 7 M2 17 9 24 17 17" fill="none" stroke="#2e2a2c" stroke-width="1" opacity=".19"/>
  </pattern>
  <pattern id="knit" patternUnits="userSpaceOnUse" width="37" height="27">
    <path d="M-5 1 13 19 31 1 49 19 M-5 11 13 29 31 11 49 29" fill="none" stroke="#655c2c" stroke-width="6" opacity=".50"/>
    <path d="M-5 -1 13 17 31 -1 49 17" fill="none" stroke="#c3b66d" stroke-width="1.5" opacity=".58"/>
  </pattern>
  <clipPath id="leftLensClip"><circle cx="521" cy="445" r="65"/></clipPath>
  <clipPath id="rightLensClip"><circle cx="723" cy="445" r="65"/></clipPath>
  <clipPath id="jacketClip">
    <path d="M320 540 C278 568 245 588 226 627 C200 676 227 732 270 761 C297 781 341 774 364 757 L397 815 C466 848 540 858 634 866 C764 867 839 831 905 756 C923 731 933 694 955 667 C1005 666 1056 638 1054 571 C1026 567 1000 550 984 522 C972 550 952 570 920 578 L913 537 C822 572 729 592 623 594 C510 591 401 570 320 540 Z"/>
  </clipPath>
</defs>
"##);
}


// Seis patas segmentadas. Desenhadas ANTES do corpo para ficarem atras dele.
fn legs(svg: &mut String) {
    svg.push_str(r##"
<g stroke="#171924" stroke-width="14" stroke-linejoin="round" stroke-linecap="round">
  <!-- Lado esquerdo: perna exterior e articulacao -->
  <path d="M294 752 L270 790 Q256 817 270 878 Q281 926 302 951 Q313 962 315 940 L312 832 L349 788 Z" fill="url(#leg)"/>
  <path d="M309 787 L277 768 L301 744 L356 779 L313 818 Z" fill="#f04d05"/>
  <path d="M365 789 Q354 811 346 840 Q354 886 407 944 Q428 967 421 940 L400 846 L445 818 Z" fill="url(#leg)"/>
  <path d="M360 801 L398 820 L443 797" fill="none"/>
  <path d="M480 808 Q463 843 485 897 Q508 954 529 951 Q542 943 535 916 L536 836 Z" fill="url(#leg)"/>
  <!-- Lado direito -->
  <path d="M708 824 L706 937 Q707 961 722 948 Q765 914 776 865 L778 808 Z" fill="url(#leg)"/>
  <path d="M840 789 L889 833 Q907 851 889 889 Q863 929 835 948 Q816 961 829 927 L851 845 L810 807 Z" fill="url(#leg)"/>
  <path d="M823 807 L851 846 L900 831" fill="none"/>
  <path d="M908 761 L974 804 Q986 838 979 890 Q971 939 949 950 Q934 960 935 932 L932 834 L868 787 Z" fill="url(#leg)"/>
  <path d="M870 779 L932 832 L974 801" fill="none"/>
</g>
"##);
}

fn carapace(svg: &mut String) {
    // Dentes ao longo do semicirculo: contorno angular suavizado, sem usar bitmap.
    svg.push_str(r##"
<path d="
 M316 538
 Q314 525 307 516 Q296 498 310 487 Q320 480 342 477
 Q347 471 338 452 Q327 431 335 424 Q341 420 363 422
 Q382 424 378 406 Q370 381 376 372 Q381 366 401 374
 Q423 384 426 366 L427 344 Q427 322 443 328
 Q470 344 476 334 Q481 325 487 306
 Q494 287 503 297 L520 311 Q531 319 537 303
 L548 283 Q558 266 568 283 L586 298
 Q598 305 607 287 Q622 260 633 277 L650 298
 Q659 305 670 290 Q688 268 699 280 Q705 286 710 302
 Q716 316 730 310 L743 299 Q762 288 766 306
 L771 325 Q775 340 790 335 L803 328
 Q823 321 824 343 L823 360 Q823 378 841 372
 L858 369 Q878 365 873 386 L869 403
 Q868 419 886 420 L902 421 Q918 420 914 438
 L907 460 Q900 477 916 479 L935 485
 Q952 491 941 508 L929 536
 L905 556 Q766 605 627 607 Q467 596 340 551 Z" fill="url(#shell)"/>

<!-- Borda interior dourada do casco. -->
<path d="M330 538 C357 413 473 320 594 320 C756 304 876 412 920 536" fill="none" stroke="#ff9018" stroke-width="12" opacity=".82"/>
<path d="M339 538 C378 424 489 337 597 330 C756 313 864 422 908 536" fill="none" stroke="#ee4500" stroke-width="3" opacity=".52"/>
<!-- Brilho lateral discreto em curva -->
<path d="M800 454 Q779 501 796 565" fill="none" stroke="#feaf39" stroke-width="11" opacity=".34"/>
<!-- Linha sob o rosto / topo do casaco -->
<path d="M319 535 C431 568 545 594 624 595 C756 600 855 563 930 535" fill="none" stroke="#171924" stroke-width="14" stroke-linecap="round"/>
"##);
}

fn raised_claw(svg: &mut String) {
    svg.push_str(r##"
<!-- Braco da pince erguida, atras da manga direita -->
<path d="M955 577 Q983 560 991 531 C978 515 970 488 973 457 C976 419 990 393 1018 384 Q1009 428 1030 459 C1046 426 1047 393 1046 353 C1088 373 1110 414 1110 462 C1109 506 1082 540 1044 542 L1048 566 Q1028 600 983 591 Z"
  fill="url(#claw)" stroke="#171924" stroke-width="14" stroke-linejoin="round" stroke-linecap="round"/>
<!-- Dedos da pinca -->
<path d="M985 494 Q1000 471 1022 460 Q1038 451 1043 426" fill="none" stroke="#171924" stroke-width="13" stroke-linecap="round"/>
<path d="M986 514 C1005 531 1029 538 1049 531 Q1086 521 1098 486" fill="none" stroke="#bf360a" stroke-width="13" opacity=".62"/>
<path d="M976 539 Q997 567 1048 575" fill="none" stroke="#171924" stroke-width="13"/>
<path d="M989 536 Q1010 551 1042 551" fill="none" stroke="#b9330a" stroke-width="6" opacity=".6"/>
"##);
}

fn blazer_base(svg: &mut String) {
    svg.push_str(r##"
<!-- Silhueta do paleto, mangas e parte inferior -->
<path d="M320 540 C278 568 245 588 226 627 C200 676 227 732 270 761 C297 781 341 774 364 757 L397 815 C466 848 540 858 634 866 C764 867 839 831 905 756 C923 731 933 694 955 667 C1005 666 1056 638 1054 571 C1026 567 1000 550 984 522 C972 550 952 570 920 578 L913 537 C822 572 729 592 623 594 C510 591 401 570 320 540 Z"
  fill="url(#wool)" stroke="#171924" stroke-width="15" stroke-linejoin="round"/>
<g clip-path="url(#jacketClip)">
  <rect x="198" y="521" width="870" height="352" fill="url(#herringbone)"/>
  <path d="M219 679 C243 612 281 576 324 558" stroke="#9f694b" stroke-width="13" opacity=".25" fill="none"/>
  <path d="M931 635 Q1013 645 1046 580" stroke="#af7c57" stroke-width="15" opacity=".3" fill="none"/>
  <path d="M907 798 Q957 745 964 686" stroke="#372c2e" stroke-width="27" opacity=".25" fill="none"/>
</g>
<!-- Costura das mangas -->
<path d="M296 643 Q315 657 344 691" fill="none" stroke="#171924" stroke-width="12" stroke-linecap="round"/>
<path d="M327 570 C339 572 347 579 352 584" fill="none" stroke="#171924" stroke-width="9" stroke-linecap="round"/>
<path d="M952 660 Q978 611 1023 619" fill="none" stroke="#171924" stroke-width="10"/>
<path d="M223 689 Q226 717 245 735" fill="none" stroke="#a15d34" stroke-width="10" opacity=".45"/>
"##);
}

fn shirt_vest_tie(svg: &mut String) {
    svg.push_str(r##"
<!-- Malha do colete, sob os lapelas -->
<path d="M481 576 L552 617 L633 688 L714 616 L765 577 L754 838 Q693 859 634 863 Q573 862 535 853 Z"
  fill="url(#vest)" stroke="#443b2d" stroke-width="9" stroke-linejoin="round"/>
<!-- Bordas caneladas em V -->
<path d="M493 593 L628 741 Q633 748 638 741 L763 593" fill="none" stroke="#6d6230" stroke-width="26" stroke-linejoin="round"/>
<path d="M493 593 L628 741 Q633 748 638 741 L763 593" fill="none" stroke="#bda75d" stroke-width="13" stroke-linejoin="round"/>
<path d="M491 591 L628 738 Q632 744 637 739 L763 591" fill="none" stroke="#4f4c28" stroke-width="2" opacity=".7"/>
<!-- Tramas individuais do decote -->
<g stroke="#6e5f2d" stroke-width="6" opacity=".73" stroke-linecap="round">
  <path d="M518 628 l-7 25 M540 653 l-7 24 M563 676 l-7 24 M586 698 l-7 25 M610 723 l-6 23"/>
  <path d="M745 619 l6 29 M724 646 l7 27 M701 673 l7 27 M680 698 l7 26 M656 721 l7 22"/>
</g>
<!-- Camisa: triangulos brancos por baixo da gravata -->
<path d="M491 578 L577 593 L624 622 L579 679 Q562 674 545 652 Z" fill="url(#whiteShirt)" stroke="#9d8963" stroke-width="6"/>
<path d="M762 576 L681 594 L637 622 L678 672 Q692 667 715 639 Z" fill="url(#whiteShirt)" stroke="#9d8963" stroke-width="6"/>
<!-- Sobreposicao das pontas da gola -->
<path d="M490 578 Q534 589 576 602 L613 623 L585 656 Q579 663 571 654 Z" fill="#fff8e8"/>
<path d="M762 577 Q718 588 684 600 L645 626 L677 656 Q682 662 690 650 Z" fill="#fff8e8"/>
<path d="M578 656 Q586 671 592 648" fill="none" stroke="#cebda1" stroke-width="5"/>
<path d="M678 656 Q670 666 664 644" fill="none" stroke="#cebda1" stroke-width="5"/>
<!-- Gravata vermelha vinho, com no e padrao de tecido -->
<path d="M620 637 L650 636 L660 703 L635 730 L603 703 Z" fill="url(#tie)" stroke="#461d29" stroke-width="8"/>
<path d="M601 623 Q613 604 625 605 Q644 604 660 624 L646 651 L617 653 Z" fill="#7d2940" stroke="#451a2a" stroke-width="8" stroke-linejoin="round"/>
<path d="M633 653 L611 710 L626 725" fill="#a83b52" opacity=".4"/>
<path d="M612 620 L625 608 L647 616 L636 643 Z" fill="#a74d59" opacity=".25"/>
<path d="M550 818 Q636 833 703 817" fill="none" stroke="#514a29" stroke-width="9"/>
"##);
}

fn blazer_lapels(svg: &mut String) {
    svg.push_str(r##"
<!-- Lapela esquerda, ombro e lateral do blazer -->
<path d="M320 540 L487 580 L526 635 L483 638 L529 738 Q556 785 566 851 C504 851 445 832 393 805 L363 747 Q341 683 346 596 L329 574 Z"
 fill="url(#wool)" stroke="#171924" stroke-width="12" stroke-linejoin="round"/>
<path d="M320 540 L487 580 L526 635 L483 638 L529 738 Q556 785 566 851 C504 851 445 832 393 805 L363 747 Q341 683 346 596 L329 574 Z" fill="url(#herringbone)"/>
<path d="M438 569 L448 623" stroke="#332b2b" stroke-width="8"/>
<path d="M491 583 L522 634 L484 639 L529 737" fill="none" stroke="#171924" stroke-width="11" stroke-linecap="round"/>
<!-- Lapela direita -->
<path d="M813 567 L918 538 L928 567 L912 580 L829 611 L834 649 L778 671 L722 803 L700 863 Q763 854 824 824 L900 768 L930 675 L925 629 Z"
 fill="url(#wool)" stroke="#171924" stroke-width="12" stroke-linejoin="round"/>
<path d="M813 567 L918 538 L928 567 L912 580 L829 611 L834 649 L778 671 L722 803 L700 863 Q763 854 824 824 L900 768 L930 675 L925 629 Z" fill="url(#herringbone)"/>
<path d="M818 569 L830 615 L834 649 L783 668 L720 800" fill="none" stroke="#171924" stroke-width="11" stroke-linejoin="round"/>
<!-- Manga direita passa por cima do braco -->
<path d="M918 580 Q958 571 986 525 Q1006 559 1053 573 Q1051 632 999 655 Q967 670 924 669 L917 644 Z" fill="url(#wool)"/>
<path d="M918 580 Q958 571 986 525 Q1006 559 1053 573 Q1051 632 999 655 Q967 670 924 669 L917 644 Z" fill="url(#herringbone)" opacity=".6"/>
<path d="M918 580 Q958 571 986 525 Q1006 559 1053 573 Q1051 632 999 655 Q967 670 924 669" fill="none" stroke="#171924" stroke-width="15" stroke-linecap="round" stroke-linejoin="round"/>
<path d="M954 663 Q977 619 1024 621" fill="none" stroke="#372c2b" stroke-width="8" opacity=".7"/>
<!-- Detalhe da barra inferior -->
<path d="M563 852 Q633 870 702 861" stroke="#171924" stroke-width="12" fill="none"/>
<!-- Bolso horizontal e lapela -->
<path d="M789 701 L869 686 L872 706 L786 722 Z" fill="#80583d" stroke="#171924" stroke-width="7" stroke-linejoin="round"/>
<!-- Caneta azul e detalhe dourado -->
<path d="M822 699 L839 647 Q846 636 851 646 L852 696" fill="#103c60" stroke="#171924" stroke-width="7" stroke-linejoin="round"/>
<path d="M835 656 L839 648" fill="none" stroke="#ffc05a" stroke-width="5" stroke-linecap="round"/>
<path d="M827 693 L840 659" fill="none" stroke="#eab75b" stroke-width="4"/>
"##);
}

fn left_book_and_claw(svg: &mut String) {
    svg.push_str(r##"
<!-- Ponta da manga esquerda: caranguejo segurando livro -->
<path d="M273 752 Q267 724 285 695 Q307 663 348 675 L382 701 Q367 746 343 784 L292 783 Z" fill="url(#claw)" stroke="#171924" stroke-width="13"/>
<path d="M333 676 Q309 665 285 686 Q260 711 260 744" fill="none" stroke="#fff2ca" stroke-width="17" stroke-linecap="round"/>
<path d="M333 674 Q308 668 286 690" fill="none" stroke="#171924" stroke-width="8" stroke-linecap="round"/>
<!-- Fitas marcadoras que saem do livro -->
<path d="M421 815 L421 873 L442 853 L462 875 L463 813" fill="#8e2330" stroke="#541e28" stroke-width="5" stroke-linejoin="round"/>
<path d="M429 819 L430 860 L442 849 L454 862 L455 823" fill="#f6bd66"/>
<!-- Capa traseira e lombada -->
<path d="M365 637 Q375 614 396 611 L553 636 L552 824 L379 775 Q363 768 363 745 Z" fill="#263d50" stroke="#171924" stroke-width="14" stroke-linejoin="round"/>
<path d="M381 622 L402 615 L548 638 L547 651 L394 637" fill="#f7efdb"/>
<path d="M382 623 L400 621 L538 645" fill="none" stroke="#d7a454" stroke-width="5"/>
<path d="M380 630 L399 638 L400 746 Q385 741 379 735 Z" fill="#375b6e"/>
<!-- Miolo e folha na borda superior -->
<path d="M390 627 L531 648 L540 655 L540 822 L395 786 Z" fill="#fff2d7" stroke="#171924" stroke-width="13" stroke-linejoin="round"/>
<!-- Capa frontal -->
<path d="M399 636 L521 664 L522 832 L398 799 Q390 796 392 787 L392 645 Z" fill="url(#bookCover)" stroke="#171924" stroke-width="13" stroke-linejoin="round"/>
<path d="M396 642 L399 746" fill="none" stroke="#2a6c73" stroke-width="5" opacity=".6"/>
<!-- Garra esquerda sobre a capa: curvas superiores e inferior -->
<path d="M289 699 Q307 686 328 700 Q350 721 382 738 Q419 759 467 744 Q473 742 471 753 Q464 780 433 799 Q404 822 362 814 Q322 809 301 789 Q283 771 283 742 Q282 716 289 699 Z"
 fill="url(#claw)" stroke="#171924" stroke-width="14" stroke-linejoin="round"/>
<path d="M304 712 Q288 741 306 764 Q326 792 362 797 Q409 802 447 775" fill="none" stroke="#c5390b" stroke-width="14" opacity=".66" stroke-linecap="round"/>
<path d="M292 704 Q312 684 331 697" fill="none" stroke="#ff7b19" stroke-width="6"/>
<!-- Parte superior da capa fica visivel atras da garra -->
<path d="M473 751 Q476 757 467 772" fill="none" stroke="#171924" stroke-width="9"/>
"##);
}

fn face(svg: &mut String) {
    svg.push_str(r##"
<!-- Hastes arredondadas -->
<rect x="431" y="426" width="31" height="29" rx="14" fill="#171924"/>
<rect x="786" y="425" width="32" height="30" rx="14" fill="#171924"/>
<!-- Aros, ponte e vidro dos oculos -->
<circle cx="521" cy="445" r="69" fill="url(#lens)" stroke="#171924" stroke-width="13"/>
<circle cx="723" cy="445" r="69" fill="url(#lens)" stroke="#171924" stroke-width="13"/>
<path d="M589 433 Q626 418 656 436" fill="none" stroke="#171924" stroke-width="13" stroke-linecap="round"/>
<!-- Pupilas ovais, nao centralizadas -->
<ellipse cx="528" cy="448" rx="27" ry="35" fill="#354454"/>
<ellipse cx="726" cy="448" rx="27" ry="35" fill="#354454"/>
<path d="M506 423 Q524 405 541 417" fill="none" stroke="#72808a" stroke-width="6" opacity=".3"/>
<!-- Reflexos em diagonal, recortados nos vidros -->
<g clip-path="url(#leftLensClip)">
  <path d="M537 363 L567 373 L468 499 L454 488 Z" fill="#fff1d7" opacity=".54"/>
  <path d="M564 383 L572 399 L497 512 L481 502 Z" fill="#fff9de" opacity=".33"/>
  <path d="M467 420 Q494 386 535 388" fill="none" stroke="#fff9e6" stroke-width="5" opacity=".16"/>
</g>
<g clip-path="url(#rightLensClip)">
  <path d="M739 367 L761 378 L667 500 L655 484 Z" fill="#fff3dc" opacity=".55"/>
  <path d="M765 382 L780 395 L693 515 L677 505 Z" fill="#fff7df" opacity=".32"/>
</g>
<!-- Brilho circular das pupilas -->
<ellipse cx="519" cy="436" rx="10" ry="11" fill="#fffbe9"/>
<ellipse cx="714" cy="436" rx="10" ry="11" fill="#fffbe9"/>
<!-- Linha fina para devolver nitidez ao aro -->
<circle cx="521" cy="445" r="69" fill="none" stroke="#171924" stroke-width="13"/>
<circle cx="723" cy="445" r="69" fill="none" stroke="#171924" stroke-width="13"/>
<!-- Sorriso em curva grossa -->
<path d="M589 505 Q624 532 660 505" fill="none" stroke="#171924" stroke-width="13" stroke-linecap="round"/>
"##);
}

fn details(svg: &mut String) {
    svg.push_str(r##"
<!-- Vincos e sombras finais que unem os elementos -->
<path d="M228 642 Q239 618 251 613" fill="none" stroke="#9b603e" stroke-width="5" opacity=".58"/>
<path d="M314 766 L334 785" fill="none" stroke="#d63c07" stroke-width="5"/>
<path d="M844 805 Q872 780 886 757" fill="none" stroke="#211c25" stroke-width="5" opacity=".55"/>
<path d="M482 591 Q488 607 496 614" fill="none" stroke="#aa8060" stroke-width="4" opacity=".4"/>
"##);
}
