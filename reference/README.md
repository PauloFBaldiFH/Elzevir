# reference/

Código de **referência de leitura**, fora do workspace Cargo. Nada aqui é compilado, empacotado ou distribuído com o Atrium.

| Caminho | O que é | Versão fixada |
|---|---|---|
| `koha/` | Koha upstream ([Koha-Community/Koha](https://github.com/Koha-Community/Koha)), GPLv3, como submódulo git raso | tag `v26.05.05-1`, commit `4f25ee4dced64026e546e047a0b7cbb371902e10` (`Koha.pm`: `26.05.05.000`) |
| `bibframe/bibframe.rdf` | Ontologia BIBFRAME da Library of Congress ([id.loc.gov/ontologies/bibframe.rdf](https://id.loc.gov/ontologies/bibframe.rdf)), CC0, cópia sem alteração | 3.0.1 (2025-12-03), SHA-256 `bac76ca817071521f8ef6e49a71cc2044d4ff4b019e7b7fefa1f5847ec6c69ca` |
| `loc-vocabularies/*.skosrdf.nt.gz` | Vocabulários da Library of Congress em lote ([id.loc.gov/download](https://id.loc.gov/download/)): `cataloging`, `languages` e `countries`, domínio público, cópia sem alteração | baixados em 2026-11-15; SHA-256 em `data/loc/vocabularies.json` |

Para baixar:

```sh
git submodule update --init --depth 1 reference/koha
```

Para trocar de versão, num PR próprio:

1. aponte o submódulo para a nova tag e atualize `koha-lock.json` (versão, série, tag e commit);
2. rode `perl tools/koha-extractor/extract.pl` para regenerar `data/koha-<série>/`;
3. rode `cargo xtask codegen` para regenerar `crates/atrium-data/src/generated/`;
4. revise o diff dos JSON: cada mudança de comportamento do Koha aparece ali.

O CI confere os passos 2 e 3 (`extract.pl --check` com o submódulo, e `cargo xtask codegen --check`). A versão-alvo é a estável mais recente do Koha (ver [docs/arquitetura/04-decisoes.md](../docs/arquitetura/04-decisoes.md), §3).

## Ontologia BIBFRAME

Para trocar de versão, num PR próprio ([spec](../docs/specs/catalogo/bibframe-completo.md), item 5):

1. substitua `bibframe/bibframe.rdf` pelo arquivo publicado e atualize a linha da tabela acima;
2. rode `perl tools/bibframe-extractor/extract.pl` para regenerar `data/loc/bibframe.json`;
3. rode `cargo xtask codegen`;
4. rode os testes: os de cobertura dizem que propriedade nova ficou sem lugar.

## Vocabulários da LC

Para atualizar, num PR próprio: baixe de novo os três arquivos `*.skosrdf.nt.gz` de https://id.loc.gov/download/vocabulary/, troque os de `loc-vocabularies/`, rode `perl tools/bibframe-extractor/vocabularies.pl` e `cargo xtask codegen`. Vocabulário novo faz o teste `every_scheme_is_used_by_a_property_or_set_aside` falhar até alguém decidir onde ele entra.
