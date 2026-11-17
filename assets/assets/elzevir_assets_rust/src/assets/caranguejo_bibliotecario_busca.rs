//! Ilustração original vetorial construída com formas SVG em Rust.
//! Não lê ou incorpora a imagem JPEG de referência.
use std::{fs, path::Path};

pub fn svg() -> String {
    r###"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1536 1147" width="1536" height="1147">
<defs>
 <linearGradient id="shell" x1="0" y1="0" x2="0.3" y2="1"><stop stop-color="#ff7400"/><stop offset="1" stop-color="#f45100"/></linearGradient>
 <linearGradient id="lens" x1="0" y1="0" x2="1" y2="1"><stop stop-color="#ffbe95"/><stop offset="1" stop-color="#ff926d"/></linearGradient>
 <linearGradient id="glass" x1="0" y1="0" x2="1" y2="1"><stop stop-color="#e4f8fb"/><stop offset="1" stop-color="#b4d6df"/></linearGradient>
</defs>
<g stroke="#1b1c29" stroke-width="8" stroke-linecap="round" stroke-linejoin="round">
<!-- Estante de madeira: traseira, laterais e prateleiras -->
<path d="M963 108 L1368 41 L1503 65 L1503 926 L1369 970 L963 835Z" fill="#9c5935"/>
<path d="M989 130 L1343 72 L1343 927 L989 813Z" fill="#bc7b4a"/>
<path d="M1020 154 L1316 113 L1316 879 L1020 790Z" fill="#78432e"/>
<path d="M963 335 L1368 334 L1368 359 L963 362Z" fill="#b47a48"/>
<path d="M963 573 L1368 573 L1368 603 L963 595Z" fill="#ad7047"/>
<path d="M1368 41 L1503 65 L1503 926 L1369 970Z" fill="#91502f"/>
<path d="M1368 41 L1368 970" fill="none"/>
<!-- Livros da prateleira superior -->
<path d="M1014 156 L1048 151 L1048 333 L1014 333Z" fill="#c69054"/>
<path d="M1050 151 L1081 147 L1125 309 L1090 317Z" fill="#69a45b"/>
<path d="M1082 147 L1154 150 L1194 310 L1125 309Z" fill="#4d934f"/>
<path d="M1224 165 L1268 164 L1268 334 L1224 334Z" fill="#55a9b8"/>
<path d="M1268 130 L1307 124 L1307 335 L1268 335Z" fill="#3fa26b"/>
<path d="M1307 123 L1340 119 L1340 335 L1307 335Z" fill="#ffd05d"/>
<!-- Livros centrais -->
<path d="M1014 390 L1052 385 L1052 558 L1014 550Z" fill="#377a82"/>
<path d="M1052 385 L1137 380 L1137 549 L1052 558Z" fill="#2f7880"/>
<path d="M1052 385 L1137 380" fill="none" stroke="#e4ebd5" stroke-width="4"/>
<path d="M1268 421 L1302 417 L1302 584 L1268 580Z" fill="#ff8461"/>
<path d="M1302 417 L1342 416 L1342 587 L1302 584Z" fill="#d2a169"/>
<!-- Livros da prateleira inferior -->
<path d="M1015 652 L1045 646 L1045 825 L1015 816Z" fill="#d0a16d"/>
<path d="M1045 645 L1076 647 L1076 833 L1045 825Z" fill="#2da45d"/>
<path d="M1076 647 L1107 644 L1107 841 L1076 833Z" fill="#ffd35a"/>
<path d="M1107 644 L1193 655 L1193 845 L1107 841Z" fill="#e7ad39"/>
<path d="M1229 702 L1264 699 L1264 870 L1229 860Z" fill="#fca062"/>
<path d="M1264 700 L1302 698 L1302 881 L1264 870Z" fill="#5baac4"/>
<path d="M1302 699 L1341 697 L1341 891 L1302 881Z" fill="#39838b"/>
</g>
<!-- Patas do caranguejo, atrás do corpo -->
<g stroke="#1b1c29" stroke-width="12" stroke-linejoin="round" stroke-linecap="round">
<path d="M180 789 L89 861 Q73 889 88 946 L121 1012 Q136 1032 140 1002 L138 888 L223 831Z" fill="#f65b00"/>
<path d="M230 823 L187 903 Q186 940 242 1019 Q262 1040 266 1010 L241 916 L295 853Z" fill="#f65b00"/>
<path d="M322 847 L284 922 Q289 968 333 1018 Q352 1038 355 1004 L343 915 L379 862Z" fill="#f65b00"/>
<path d="M393 865 L406 960 Q418 1026 455 1026 Q477 1024 471 1000 L460 874Z" fill="#f65b00"/>
<path d="M618 856 L613 1008 Q615 1030 634 1020 Q686 984 706 902 L679 855Z" fill="#f65b00"/>
<path d="M697 834 L709 901 L710 1004 Q719 1032 741 1004 Q778 947 789 886 L751 824Z" fill="#f65b00"/>
<path d="M755 806 L833 871 L822 1000 Q822 1025 839 1014 Q885 965 885 872 L794 795Z" fill="#f65b00"/>
<path d="M775 807 L863 870" fill="none"/>
<path d="M704 876 L758 917 M205 867 L245 904 M296 889 L338 912" fill="none"/>
</g>
<!-- Corpo do caranguejo e contorno serrilhado -->
<path d="M167 591 Q157 580 171 566 L194 551 Q180 532 172 517 Q165 505 181 499 L207 493 Q201 472 193 454 Q191 443 210 441 L237 438 Q236 418 235 399 Q239 385 255 388 L287 390 Q288 365 291 345 Q298 334 313 344 L340 361 Q349 342 349 316 Q355 302 370 313 L400 333 Q415 312 418 285 Q428 270 444 284 L470 311 Q486 294 500 269 Q511 251 524 270 L550 304 Q570 286 585 268 Q596 256 609 276 L632 312 Q654 302 670 289 Q687 278 691 301 L701 330 Q726 321 742 314 Q756 309 760 332 L765 360 Q791 354 804 350 Q823 345 821 367 L820 396 Q842 400 858 402 Q875 408 864 425 L852 446 Q870 456 883 468 Q894 479 880 492 L863 512 Q878 529 885 543 Q891 557 873 566 L852 579 Q864 602 853 621 Q836 646 803 660 L744 698 L240 696 L179 640Z" fill="url(#shell)" stroke="#1b1c29" stroke-width="11" stroke-linejoin="round"/>
<path d="M184 567 Q268 372 458 379 Q652 373 735 528" fill="none" stroke="#ff931e" stroke-width="11" stroke-linecap="round" opacity=".85"/>
<!-- Sombra lateral do casco -->
<path d="M751 484 Q823 582 788 652 L747 693 L703 696 Q771 627 751 484Z" fill="#e84b00" opacity=".65"/>
<!-- Blazer e braços -->
<g stroke="#1b1c29" stroke-width="11" stroke-linejoin="round" stroke-linecap="round">
<path d="M170 610 Q94 649 109 764 Q126 843 218 860 L435 927 L625 924 Q744 907 789 836 Q815 789 780 699 L753 624 Q671 680 458 686 Q273 682 170 610Z" fill="#79594b"/>
<path d="M177 633 Q109 681 139 770 Q160 806 220 823 L312 845 L314 703Z" fill="#74554b"/>
<path d="M751 644 Q784 684 790 765 Q777 851 666 895 L626 916 L649 705Z" fill="#725147"/>
<!-- camisa e suéter -->
<path d="M416 685 L469 742 L520 765 L575 728 L614 675 L649 708 L626 922 L461 929 L449 800Z" fill="#a39a46"/>
<path d="M414 687 L467 730 L518 760 L570 727 L613 683 L591 744 L520 812 L449 746Z" fill="#f8f0d4"/>
<path d="M451 752 L520 818 L588 748" fill="none" stroke="#d2b96e" stroke-width="13"/>
<!-- gravata -->
<path d="M505 706 L536 708 L553 736 L534 760 L549 801 L522 825 L494 799 L510 759 L490 733Z" fill="#7d1936"/>
<path d="M510 757 L533 760" fill="none" stroke="#46152c" stroke-width="6"/>
<!-- lapelas -->
<path d="M330 654 L402 690 L439 741 L411 757 L449 813 L464 927 L316 903 L331 750 L292 717Z" fill="#78574d"/>
<path d="M645 668 L613 690 L582 745 L607 762 L578 810 L624 917 L707 883 L713 731 L675 724Z" fill="#78574d"/>
<path d="M324 678 L349 715 L320 752 L371 781 M653 684 L631 724 L670 749 L627 779" fill="none"/>
<!-- bolso e caneta -->
<path d="M682 800 L751 782 L757 807 L686 825Z" fill="#4a3432"/>
<path d="M713 783 L720 737 Q722 727 733 733 L731 790" fill="#2b7290"/>
<path d="M721 756 L723 736" fill="none" stroke="#ffd466" stroke-width="5"/>
</g>
<!-- Braço com lupa -->
<g stroke="#1b1c29" stroke-width="11" stroke-linecap="round" stroke-linejoin="round">
<path d="M775 692 Q809 602 868 599 Q916 599 929 652 Q944 708 893 747 Q845 777 805 734Z" fill="#f65a00"/>
<path d="M857 684 L904 575" fill="none" stroke-width="26"/>
<path d="M857 684 L904 575" fill="none" stroke="#666b6c" stroke-width="15"/>
<circle cx="946" cy="486" r="84" fill="#c5cfd1"/>
<circle cx="946" cy="486" r="65" fill="url(#glass)" stroke-width="6"/>
<path d="M911 460 Q930 433 957 438" fill="none" stroke="#ffffff" stroke-width="13"/>
<circle cx="906" cy="474" r="7" fill="#fff" stroke="none"/>
</g>
<!-- Rosto e óculos -->
<g stroke="#1b1c29" stroke-linecap="round" stroke-linejoin="round">
<path d="M230 559 Q269 537 354 547" fill="none" stroke-width="12"/>
<path d="M490 528 Q510 516 533 528" fill="none" stroke-width="11"/>
<path d="M679 522 L787 492 Q807 490 815 509" fill="none" stroke-width="12"/>
<circle cx="427" cy="548" r="78" fill="url(#lens)" stroke-width="13"/>
<circle cx="620" cy="534" r="78" fill="url(#lens)" stroke-width="13"/>
<path d="M352 523 L407 478 M550 512 L602 464" fill="none" stroke="#ffceb2" stroke-width="19" opacity=".7"/>
<ellipse cx="454" cy="549" rx="28" ry="43" fill="#1b1c29" stroke="none"/>
<ellipse cx="646" cy="534" rx="28" ry="43" fill="#1b1c29" stroke="none"/>
<ellipse cx="465" cy="531" rx="10" ry="13" fill="white" stroke="none"/>
<ellipse cx="654" cy="516" rx="10" ry="13" fill="white" stroke="none"/>
<path d="M506 630 Q536 611 559 632" fill="none" stroke-width="9"/>
</g>
<!-- pinça esquerda, na frente -->
<g stroke="#1b1c29" stroke-width="11" stroke-linejoin="round" stroke-linecap="round">
<path d="M218 745 Q263 735 300 774 Q339 816 374 818 Q355 863 307 877 Q254 890 211 856 Q182 832 189 794Z" fill="#f65b00"/>
<path d="M214 750 Q229 710 257 711 Q293 711 320 747 Q337 769 344 786 Q298 783 276 765 Q266 744 243 748Z" fill="#f65b00"/>
<path d="M185 736 Q198 709 226 712" fill="none" stroke="#fff6d8" stroke-width="15"/>
</g>
<!-- Interrogação -->
<text x="760" y="340" font-family="DejaVu Sans,Arial,sans-serif" font-size="170" font-weight="900" text-anchor="middle" fill="#1b1c29">?</text>
</svg>
"###.to_owned()
}

pub fn salvar_svg(destino: impl AsRef<Path>) -> std::io::Result<()> {
    fs::write(destino, svg())
}
