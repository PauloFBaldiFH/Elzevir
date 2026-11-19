# Importar e exportar pela linha de comando (v0)

> Status: v0, tarefa 1.11. Base: [doc 03](../../arquitetura/03-estrategia-reconstrucao.md) §1.2 item 10 (um binário, vários papéis), [importação](importacao.md), [casamento](casamento.md) e [mapeamento MARC21](mapeamento-marc21.md) §7. Critério da tarefa: **importação do corpus da LC e de uma base exportada de um Koha**.

## 1. Comportamento

### `elzevir import --data-dir DIR [opções] ARQUIVO`

1. Detecta pelo conteúdo, sem consumir o buffer que será importado. Lê MARC21 ISO 2709 com `import_marc21`: o mesmo código que a importação pela tela vai usar. Formato ainda não suportado ou desconhecido termina com saída 1 antes de abrir a base ou criar códigos; extensão não decide o formato. Paralelismo pelo número de núcleos; `ELZEVIR_SERIAL=import` deixa numa thread só ([importação](importacao.md), item 8).
2. **Casamento** ([casamento](casamento.md)):

| Opção | Valores | Padrão |
|---|---|---|
| `--match` | `isbn`, `issn`, `koha-biblionumber` | nenhum: grava todo registro |
| `--on-match` | `ignore`, `create-new`, `replace` | `ignore` |
| `--on-no-match` | `create-new`, `ignore` | `create-new` |
| `--items` | `always-add`, `add-only-for-matches`, `add-only-for-new`, `replace`, `ignore` | `always-add` |

   As três últimas só valem com `--match`; sem ele, a linha de comando recusa.
3. `--create-codes`: antes de importar, lê o arquivo uma vez, junta as unidades (de origem e atual) e os tipos de item dos exemplares e cria os que faltam, com o código como nome. Os que já existem ficam como estão. Sem a opção, exemplar com código desconhecido recusa o registro inteiro (sem casamento) ou só o exemplar (com casamento).
4. **Relatório**: na saída padrão, duas linhas com lidos, gravados, casados, substituídos, pulados e com falha; exemplares gravados, substituídos e recusados. Na saída de erro, uma linha por registro recusado (posição contada a partir de 1 e deslocamento em bytes) e por exemplar recusado.
5. A saída é **0** quando a importação chega ao fim, mesmo com registros recusados (eles estão no relatório). É **1** quando não dá para abrir o arquivo ou a base, ou a leitura para no meio.

### `elzevir import ETAPA --data-dir DIR --operator ID ...`

5a. O mesmo comando reúne as etapas do lote privado da [importação universal](importacao-universal.md): `stage`, `edit`, `preview`, `confirm` e `cancel`. Com uma etapa, as opções da importação direta não valem e a etapa vem logo depois de `import`. Sem etapa, vale a importação direta acima, sem mudança. Os scripts de migração do Koha continuam com a gravação direta; o lote serve para revisar e editar antes de gravar, como na tela da equipe.

### `elzevir export --data-dir DIR [--output ARQUIVO] [--format marc21|turtle|rdfxml] [--iri-base URL]`

6. Escreve todo o acervo em MARC21 ISO 2709, UTF-8, em ordem de Instância, num arquivo ou na saída padrão. A contagem vai para a saída de erro, para não se misturar ao MARC.
7. Os registros são lidos do banco **1.024 de cada vez** (`RecordSource::records_after`), então a memória não cresce com o acervo.
8. **Registro que ninguém mudou sai com os bytes guardados.** Quando o modelo ainda diz o mesmo que o registro de origem mapeado (`marc_to_bibframe`), a exportação escreve o registro de origem como está. Registro mudado passa por `bibframe_to_marc` a partir do de origem ([mapeamento MARC21](mapeamento-marc21.md) §7). Isso cobre registros que os leitores aceitam mas a escrita ISO 2709 recusa: 8 dos 250.000 registros da LC têm um delimitador de subcampo dentro do `001`.
9. Registro sem origem (catalogado no Elzevir) é escrito só a partir do modelo. Registro que não cabe em ISO 2709 é relatado e pulado.
9a. Com `--format turtle` ou `--format rdfxml`, escreve o acervo em BIBFRAME, um documento só, com os IRIs sob `--iri-base` (obrigatório nesses formatos): ver [BIBFRAME completo](bibframe-completo.md), item 28.

### `elzevir koha-codes --data-dir DIR --url URL --user USUÁRIO`

Traz do Koha os nomes de unidades, tipos de item, localizações e coleções: ver [tabelas de códigos](tabelas-de-codigos.md).

### Trava da pasta de dados

10. Todo papel que abre a base (`serve`, `staff`, `import`, `export`) trava `DIR/elzevir.lock` com uma trava exclusiva do sistema (`File::try_lock`) até terminar. Um segundo processo na mesma pasta para com "another Elzevir process is using DIR". Assim só existe uma escritora por base, como manda o desenho. Importar com o servidor no ar fica para a importação pela tela, que roda dentro do servidor. A trava some com o processo, mesmo se ele cair.

## 2. Origem no Koha

- Importação: `misc/migration_tools/bulkmarcimport.pl` (opções de casamento `-match`, `-insert`, `-update`) e a tela `tools/stage-marc-import.pl`. O Elzevir usa os nomes de ação da tela (`_get_commit_action`) para as duas.
- Exportação: `misc/export_records.pl` e `tools/export.pl`. O Koha regera o MARC do `biblio_metadata` e acrescenta os exemplares (952) e o `999`. O Elzevir guarda o registro de origem com os exemplares e reescreve só o que mudou.

## 3. Paridade

`livre` (a exportação MARC está na lista de paridade exigida, mas o que ela exige é o conteúdo do registro, coberto pela ida e volta do mapeamento; a forma de chamar é livre). Prova:

- `crates/elzevir/tests/cli.rs`, rodando o binário:
  - a exportação do Koha (200 registros, 253 exemplares) entra com `--create-codes` e sai **byte a byte igual**, em arquivo e na saída padrão;
  - reimportar com `--match koha-biblionumber` não grava nada e recusa os 253 exemplares;
  - sem `--create-codes`, exemplares com unidade desconhecida são recusados;
  - `--on-match` sem `--match` é recusado;
  - pasta travada por outro processo é recusada, e liberada volta a funcionar.
- `crates/elzevir-store/tests/export.rs`: ida e volta byte a byte em lotes de 1.024 e de 7; exemplar acrescentado depois da importação aparece na exportação; o registro da LC com delimitador no `001` sai igual.

**Corpus da LC** (medido em 2026-11-13, contêiner de 4 núcleos, build `release`): `BooksAll.2016.part01.utf8` do MDSConnect da Library of Congress, 250.000 registros, 242 MB, sem exemplares.

| Passo | Resultado |
|---|---|
| `import` | 250.000 gravados, 0 falhas, 94 s (cerca de 2.650 registros/s); base de 1,7 GB |
| `export` | 250.000 escritos, 0 falhas, 31 s; arquivo **byte a byte igual** ao original |
| `import --match isbn --on-no-match ignore` | 0 gravados, 172.016 casados, 77.984 pulados (sem ISBN), 13 s |

O arquivo da LC não entra no repositório (242 MB); só o registro com delimitador no `001` virou fixture.

## 4. Fora desta versão

- Importar MARCXML e UNIMARC pela linha de comando (a leitura existe; falta o papel).
- Exportar só uma parte do acervo (por unidade, por data, por lista de ids), como `export_records.pl`.
- Acrescentar o `999 $c` com o id do Elzevir na exportação.
- Importação pela tela, no servidor.

## 5. Compatibilidade

O contrato v0 acima continua descrevendo os comandos disponíveis. Os novos
formatos e opções entram por PRs separados; a importação ISO 2709 direta,
suas ações de casamento, códigos de saída e trava da pasta são preservados.

## 6. Prévia da importação universal

`elzevir import --data-dir DIR --preview ARQUIVO` aceita ISO 2709, MARCXML
e MarcEdit UTF-8. A prévia consulta o catálogo em modo de leitura, apresenta
até três amostras e retorna saída 1 se um problema exigir revisão.
`--create-codes` é incompatível com a prévia. A importação direta continua
aceitando ISO 2709. Veja a [spec da importação universal](importacao-universal.md).
