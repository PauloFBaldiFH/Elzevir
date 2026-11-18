# Importação universal: preparação, revisão e confirmação

> Status: implementação incremental I0–I10; I2 acrescenta detecção à CLI,
> preservando ISO 2709. I3 acrescenta MARCXML à aplicação e I4 acrescenta
> MarcEdit `.mrk`. I5 entrega a preparação integral MARC por `--preview`.
> I6 acrescenta lotes MARC privados, edições e confirmação por revisão.
> I7 oferece a revisão desses lotes na interface autenticada da equipe.
> I8 acrescenta preparação e confirmação de CSV e TSV nativos pela CLI e
> pela interface da equipe, com um modelo BIBFRAME gerado pela aplicação.
> A importação direta pela CLI mantém o contrato de I2.
> Base: [importação atual](importacao.md), [casamento](casamento.md),
> [BIBFRAME completo](bibframe-completo.md),
> [ADR da importação universal](../../adr/0004-importacao-universal.md) e
> [contrato da tela](../../UI_BIBFRAME_INTERACTION_SPEC.md#21-importação-universal-na-interface-da-equipe).
> Cada PR I0–I10 depende da mesclagem do anterior por Paulo.

## 1. Comportamento

### 1.1 Entrada única e caminho de gravação

A equipe envia um arquivo, confere a detecção, prepara o lote inteiro, corrige
os registros, revê a prévia e confirma a importação. A tela é web, em rota
própria da equipe, dentro de `elzevir serve`. A CLI chama as mesmas funções
de `elzevir-app`; não inicia outro processo de importação dentro do servidor.

Todo registro chega a `RecordSink::submit(NewRecord, Option<Matching>)`, que
usa `AddRecord` ou `ImportRecord`. A escritora consulta, decide o casamento
e grava na transação existente. A prévia não autoriza uma decisão calculada
fora dela. Não há outra conexão de escrita, SQL direto no importador, novo
comando alternativo de gravação, nem tabelas SQLite para preparar o lote.

`import_marc21` conserva assinatura, comportamento e `ImportReport`.
O fluxo ISO 2709 direto continua aceitando os mesmos arquivos, opções,
recusas por registro e resultados parciais. Uma função geral de importação
pode ficar ao lado dele depois do digest fixo de I1; qualquer delegação
interna depende de manter esse teste e os anteriores sem alteração.

### 1.2 Formatos e detecção pelo conteúdo

| Entrada | Reconhecimento e tratamento planejados |
|---|---|
| ISO 2709 | Líder, diretório e delimitadores MARC; conserva o leitor atual. |
| MARCXML | Estrutura MARC21, namespace e elementos `collection`/`record`; leitura em fluxo, preservando ordem, repetições e indicadores. |
| MarcEdit `.mrk` | Linhas `=LDR`, campos de controle e dados; `\` representa indicador branco, `$` delimita subcampo e escapes como `{dollar}` representam texto literal. |
| CSV/TSV | Estrutura tabular consistente; separador e codificação apresentados para conferência. |
| Dump SQL parcial do Koha | Instruções e tabelas declaradas, antes de examinar XML dentro de valores; leitura de dados bibliográficos e exemplares, sem executar SQL. |
| Backup completo do Koha | Inventário de uma instância completa; recusa com explicação de que restauração é outra operação. |
| `.xlsx`/`.ods`, ZIP e gzip | Assinatura e estrutura do contêiner, com leitura limitada de seus membros; disponíveis somente em I10. |

O detector puro retorna `Iso2709`, `MarcXml`, `MarcText`, `Delimited`,
`KohaSqlDump`, `KohaFullBackup` ou `Unknown`, acompanhado da evidência e de
ambiguidades. Contêiner identificado ainda sem leitor é evidência de formato
não suportado, não de ISO 2709. Nome e extensão não decidem o formato.
No caminho ISO, usa o buffer existente (`fill_buf`), sem reler o arquivo.

Evidência insuficiente ou conflitante exige escolha manual. A troca de
formato refaz leitura e validação; não contorna limites, autoriza backup
completo nem permite tratar autoridade como registro bibliográfico.
Formato reconhecido mas ainda não implementado é recusado antes de abrir
uma base nova, criar códigos ou enviar comandos; a CLI termina com saída 1.
O detector não promete distinguir qualquer formato a partir de um prefixo.
No fluxo preparado, a validação integral confirma ou recusa a hipótese antes
de gravar; o ISO direto mantém a leitura e as recusas por registro atuais.

**Entrega I2:** `detect_format` em `elzevir-app` examina no máximo
`MAX_DETECTION_BYTES` (8 KiB), sem I/O, alocação proporcional ao arquivo ou
trechos da origem no resultado. A evidência é tipada. A CLI usa `fill_buf`
do mesmo `BufReader` entregue à importação, antes de abrir a base ou criar
códigos. A opção existente `--create-codes` mantém sua própria leitura prévia.
Arquivo vazio ou somente espaço conserva o relatório ISO zerado; líder
numérico e registros danificados continuam sob a validação do leitor atual.

MARCXML exige raiz `record`/`collection` ligada ao namespace MARC21; `.mrk`,
`=LDR`; CSV/TSV, duas linhas lógicas completas com quantidade consistente
de colunas, respeitando aspas. Empate de separadores retorna `Unknown`.
SQL exige declarações `CREATE TABLE`/`INSERT INTO`, não nomes dentro de
comentários ou valores. O conjunto `biblio_metadata`, `items`, `borrowers`,
`issues` e `systempreferences` é evidência de backup completo neste prefixo;
uma tabela privada isolada não é. Prefixo incompleto não prova o inventário
inteiro. ZIP/gzip retornam `Unknown` com evidência de contêiner reconhecido.
Demais formatos, prefixos inconclusivos e contêineres são recusados com saída
1, sem alterar a instituição. A classificação não substitui a validação do
arquivo inteiro; um futuro leitor SQL deve conferir seu inventário completo.

### 1.3 Preparar e preservar a origem

**Entrega I3:** `MarcXmlReader` lê documentos XML 1.0 UTF-8 com raiz `record` ou
`collection` no namespace MARC21, inclusive prefixado. Preserva ordem,
repetições, indicadores, espaços e texto de campos; referências XML e CDATA
viram texto UTF-8. O líder convertido declara UTF-8. DTD, entidades externas,
codificações diferentes e registros de autoridade são recusados, sem buscar
recursos pela rede ou colocar valores da origem nos diagnósticos do leitor.

Os limites de `XmlLimits` são aplicados durante a leitura, inclusive antes
de um evento XML ultrapassar seu orçamento de bytes. Além dos limites de
XML abaixo, `MAX_XML_INPUT_BYTES` = 512 MiB e `MAX_XML_RECORDS` = 100.000
reproduzem as cotas do fluxo novo; não restringem ISO 2709. Campos ausentes,
estrutura inválida ou XML truncado interrompem esse fluxo, com índice e
posição; lotes anteriormente confirmados permanecem. Falha de conversão
ISO, mapeamento ou recusa da escritora continua no relatório por registro.

`import_marcxml` usa a mesma função interna de lotes de `import_marc21`,
sem mudar sua assinatura, relatório ou decisões. O digest I1 e a exportação
byte a byte dos 200 registros são os critérios dessa extração interna.
I3 não habilita MARCXML na CLI: os testes de capacidade de I2 continuam
exigindo recusa. A exposição no comando precisa resolver esse contrato
explicitamente, preservando os testes de regressão do importador.

**Entrega I4** — `MarcTextReader` lê o mnemônico bibliográfico UTF-8 do
MarcEdit, com BOM opcional, linhas LF ou CRLF e uma linha vazia entre
registros. O último campo pode terminar no fim do arquivo sem quebra de
linha. Cada linha de dados começa com `=TAG` e dois espaços. `=LDR` inicia
um registro e fornece um líder de 24 bytes. Campos `001`–`009` trazem um
valor. Os demais trazem dois indicadores ASCII e subcampos delimitados por
`$`, cada um com um código alfanumérico ASCII. Espaços, ordem, campos e
subcampos repetidos são preservados. Entradas só com separadores ficam vazias.

No líder, nos campos de controle e nos indicadores, a barra invertida
representa um espaço. No texto de subcampos, ela é literal. Os valores
decodificam `{dollar}` como `$`, `{bsol}` como barra invertida e
`{lcub}`/`{rcub}` como chaves literais, em uma passagem após separar os
subcampos. Chaves fora desses escapes, bytes inválidos e linhas de
continuação são recusados. A etapa cobre UTF-8 e esse conjunto de escapes
ASCII. Formato `.mrk8` e mnemônicos de outros conjuntos de caracteres
ficam fora do escopo.
[Exemplos de MarcEdit](https://github.com/reeset/MarcEdit_tasks/blob/001c1f1d6061b38d4afb0d8693511d1f9a84c8e4/tests/m-020fixes_input.mrk)
· [Sintaxe no Library Carpentry](https://github.com/LibraryCarpentry/lc-marcedit/blob/9df4a60266a792f3a7077bb18a4d45ff11d0e606/episodes/03-working-with-MARC-files.md)
· [Tabela mnemônica MARCMaker](https://metacpan.org/release/EIJABB/MARC-File-MARCMaker-0.05/source/lib/MARC/File/MARCMaker.pm).

`MrkLimits` aplica os limites durante a leitura em fluxo. Bytes de linha
incluem o terminador. Bytes e linhas do registro incluem o líder e os
campos. O orçamento de entrada inclui os separadores e o BOM. Erro de
sintaxe, codificação, E/S ou limite interrompe a leitura com índice do
registro, linha física e byte, sem ecoar valores da origem. A API
`import_marc_text` serializa a origem em ISO 2709 UTF-8 e compartilha os
lotes, o mapeamento e a escritora usados por `import_marc21`. Falhas na
conversão ISO, no mapeamento ou na escritora seguem o relatório por
registro. Lotes já confirmados permanecem após interrupção. Os testes
de capacidade da CLI continuam a exigir recusa de `.mrk`.

Preparar percorre o arquivo inteiro na ordem de entrada, aplica somente as
escolhas explícitas do lote e executa o mesmo mapeamento e validação usados
na confirmação. Registra todos os erros e avisos, não apenas os da amostra.

- ISO 2709 já UTF-8 e sem edição conserva os bytes originais. MARC-8 e
  Latin-1 seguem a conversão atual, conforme [importação §1.4](importacao.md).
- MARCXML, `.mrk` e SQL produzem `Record` na borda, serializado como ISO
  2709 UTF-8 para `source`. O resultado passa por `marc_to_bibframe` sem
  alterar seu comportamento. Campos não mapeados continuam na origem.
- Não há limpeza automática de `999 $c`, `999 $d` ou `952 $9`.
  Identificadores de origem não viram chaves primárias locais; `999 $c`
  continua disponível para o casamento `KohaBiblionumber`.
- Planilha produz `NewRecord` nativo, com `source: None`; nunca passa por
  MARC. Usa a validação pública de criação/edição e dos exemplares.
  A exportação MARC posterior parte do modelo, como na catalogação nativa.

### 1.4 Mapeamento das planilhas

CSV/TSV aceita UTF-8 com ou sem BOM e Latin-1. Bytes válidos como UTF-8 usam
UTF-8; outra interpretação, separador empatado ou cabeçalho incerto exige
conferência. Não se tenta corrigir codificação silenciosamente.

O leitor tabular recebe separador e codificação explícitos. Aceita aspas
duplas, aspas duplicadas dentro de campos citados e quebras de linha dentro
desses campos. Linhas físicas vazias entre registros são ignoradas.
Campos vazios e colunas repetidas conservam suas posições. Erros informam
a linha lógica, a coluna e o byte inicial da linha no arquivo.

O gerador produz um cabeçalho UTF-8 em CSV ou TSV, com a versão do modelo
e a identidade da ontologia no início. Cada coluna de descrição qualifica
a propriedade pela entidade, seguindo as tabelas geradas da LC na ordem
das tabelas geradas para a ontologia vendorizada.

O mapeamento é um perfil TOML versionado. Cada entrada declara a posição
e o cabeçalho exato da coluna, o destino e se deve aparar espaços.
Colunas com cabeçalhos iguais são identificadas separadamente. A versão é
obrigatória; versão desconhecida e destino inválido bloqueiam a preparação.
Sem um perfil, a revisão de uma planilha própria oferece sugestões por
posição a partir dos [sinônimos publicados](../../../crates/elzevir-app/data/import-header-aliases.tsv).
O catalogador declara cada destino ou usa `target = "ignore"`.

Unidade de origem e atual, localização, coleção e política são resolvidas
por **nome** nos cadastros genéricos. Localização e coleção pertencem à
unidade escolhida. Nome ausente, desconhecido ou ambíguo exige revisão;
nunca cria código, tipo de item ou padrão institucional. Classes, conteúdo,
mídia e suporte vêm das listas oficiais da LC, sem inferência por política.
Códigos e `koha_type_statements` permanecem exclusivamente na borda MARC.

O modelo usa cabeçalhos qualificados como `Work.title` e
`Instance.identifiedBy`. As duas colunas de identidade do modelo ficam
vazias nas linhas de dados. Use `Work.@type`, `Instance.@type` e
`Item.@type` para as classes. Células de propriedades repetidas e
estruturadas usam listas JSON dos valores `Value` existentes. O título,
o resumo, o público-alvo, a edição e a série aceitam texto simples.
O destino `Instance.catalog.isbn` de um perfil aceita o ISBN em texto.
Algumas propriedades normalizadas exigem uma forma específica ou uma
identidade existente. O preparador recusa valores sem representação
compatível e a associação de Hub usa um identificador já cadastrado.
Os validadores de criação e descrição BIBFRAME conferem os valores.

Cada linha descreve uma Instância e pode levar um exemplar. O agrupamento
explícito por ISBN ou título reúne os exemplares e exige que todas as
linhas no grupo tenham a mesma descrição.
ISBN ou título coincidente não funde Obras silenciosamente. Mapeamento salvo
contém regras e cabeçalhos, nunca linhas, amostras ou valores extraídos do
arquivo. Reaplicá-lo exige conferir colunas ausentes ou incompatíveis.

Cada exemplar exige `Item.catalog.home_branch`,
`Item.catalog.holding_branch` e `Item.catalog.policy` por nome.
`Item.catalog.barcode` é obrigatório por padrão. Para aceitar ausência,
liste a posição da linha de dados em `accept_missing_barcodes`. A primeira
linha de dados vale 1 nessa lista e em `exclude`. Diagnósticos de planilha
contam o cabeçalho como linha lógica 1, inclusive se houver células
multilinha. A preparação rejeita um código de barras repetido entre as
linhas selecionadas, com as duas posições e o código. Se o catálogo mudar
depois da revisão, a escritora pode recusar o exemplar com sua posição e
seu código. O [ADR 0007](../../adr/0007-exemplares-nativos-na-importacao.md)
define a entrada nativa da escritora e a compatibilidade das gravações MARC.

#### Preparar um CSV ou TSV

Na interface autenticada da equipe, abra **Importar CSV ou TSV**,
baixe um modelo ou envie uma planilha própria. Para planilha própria,
salve o perfil no campo da receita. Revise a prévia antes de confirmar.

Na CLI, gere o cabeçalho ou prepare diretamente o arquivo preenchido.
Use `--tsv` para gerar o modelo TSV. A etapa `stage` imprime o ID do lote.

```sh
elzevir import-model > modelo.csv
elzevir import-batch --data-dir DIR --operator 1 stage preenchido.csv --table
elzevir import-batch --data-dir DIR --operator 1 preview ID
elzevir import-batch --data-dir DIR --operator 1 confirm ID 1
```

Para um arquivo próprio com a coluna `Título`, salve a receita abaixo.
Cada coluna adicional exige uma entrada `[[table.profile.columns]]`.
Os separadores são `comma`, `semicolon` e `tab`. Declare `latin1`
quando a origem usar ISO-8859-1. Para agrupar, declare `group_by = "isbn"`
ou `group_by = "title"` em `[table]`.

```toml
[table]
delimiter = "comma"
encoding = "utf8"

[table.profile]
version = 1

[[table.profile.columns]]
position = 1
header = "Título"
target = "Work.title"
trim = true
```

```sh
elzevir import-batch --data-dir DIR --operator 1 edit ID 1 receita.toml
elzevir import-batch --data-dir DIR --operator 1 preview ID
elzevir import-batch --data-dir DIR --operator 1 confirm ID 2
```

### 1.5 Editar antes da confirmação

Para origem MARC, regras em lote adicionam, removem ou substituem campos e
subcampos, ou substituem texto com filtro por etiqueta; também há edição
individual. Operações são estruturadas e ordenadas, sem execução de código.
Aplicam-se ao `Record` antes de `marc_to_bibframe`; a prévia mostra antes,
depois, alcance e problemas. A origem guardada é a versão editada em ISO
2709. Sem edição, não se reserializa ISO 2709 UTF-8 desnecessariamente.

Para planilha, a edição atua nas propriedades BIBFRAME e nos exemplares,
com os mesmos validadores da criação. Nenhuma operação inventa campos
MARC intermediários. Valores inválidos permanecem visíveis para correção.
Alterar formato, regras, mapeamento, seleção de registros, agrupamento ou
opções de casamento invalida a prévia anterior e exige nova preparação.

### 1.6 Prévia, revisão e casamento

`elzevir import --data-dir DIR --preview ARQUIVO` prepara ISO 2709, MARCXML
e MarcEdit sem gravar catálogo, auditoria, códigos ou efeitos. A tela
chama a mesma função de aplicação. Consulta ao catálogo usa só o pool de
leitura, por portas de `elzevir-app`, inclusive para cadastros e colisões.
`--preview` termina com saída 1 se houver erro ou revisão obrigatória pendente;
prévia válida, inclusive com avisos, termina com 0. O ISO direto não muda.

A prévia da CLI trata uma base ausente como catálogo vazio e exige schema
atual para consultar uma base existente. `--create-codes` é incompatível com
a prévia. Os leitores compartilham conversão e mapeamento com a importação.
A leitura do catálogo usa um snapshot pelo pool de leitura. Erros
estruturais dos formatos textuais interrompem a preparação. A CLI apresenta
até três amostras mapeadas e emite todos os diagnósticos de validação nas
entradas lidas até o fim.
Ao abrir uma base SQLite em modo WAL, a própria biblioteca pode criar
arquivos auxiliares WAL/SHM para coordenar a leitura.

As ações de casamento disponíveis na CLI também valem para a previsão.
Chaves repetidas no lote pedem revisão e apontam para o primeiro registro
nessa situação, sem pressupor o resultado de substituições ainda pendentes.
Exemplares que a ação escolhida descarta ficam fora da validação de
cadastros e de códigos de barras. Para exemplares que seriam gravados, o
código ausente ou repetido exige revisão, salvo colisão coberta pela ação
explícita de substituição.

A amostra inicial tem três registros em blocos Obra / Instância / Exemplar;
todos os demais e todos os diagnósticos são acessíveis por paginação.
Cada problema identifica posição, campo/célula, gravidade e motivo. Não
confunde linha física de CSV multilinha com o número do registro lógico.

Erro bloqueia a confirmação até correção ou exclusão explícita do registro;
aviso permanece visível para revisão e não bloqueia sozinho. Código de
barras ausente ou repetido no arquivo ou na base vai para revisão, sem
geração automática, renumeração ou descarte. Ausência pode ser aceita
explicitamente quando o modelo permite; colisão precisa ser resolvida ou
receber a ação de exemplares já admitida pelo casamento existente.

As opções são as mesmas da [CLI atual](linha-de-comando.md): sem regra,
`None`; com regra, ISBN, ISSN ou `KohaBiblionumber`, ações de encontrado,
não encontrado e exemplares. As regras de `matching.rs` não mudam.
A prévia mostra possíveis casamentos, inclusive conflitos dentro do lote;
não reserva registros nem toma a decisão final de casamento.
Para a mesma entrada, opções e estado da base, validação de prévia e de
confirmação deve produzir os mesmos erros. A escritora reavalia o estado
atual e os efeitos dos registros anteriores; concorrência pode mudar o
resultado previsto. O relatório distingue previsão de resultado efetivo.

### 1.7 Lote preparado, confirmação e resultado

Em I6 o lote fica em `<data-dir>/imports/<id>/`: original imutável,
identificação do formato e JSON versionado das edições e escolhas. O id é
opaco, gerado pelo sistema; não é um caminho enviado pelo navegador.
Permissões privadas e vínculo com instituição e operador restringem acesso.
Não se segue link simbólico nem se reutiliza nome de arquivo como caminho.

A I6 oferece `elzevir import-batch --data-dir DIR --operator ID` com
`stage ARQUIVO`, `edit LOTE REVISAO RECEITA.json`, `preview LOTE`,
`confirm LOTE REVISAO` e `cancel LOTE`. O diretório da instituição já
deve existir. A identidade do operador nessa CLI local é fornecida pelo
operador do sistema. No Windows, o diretório de dados precisa ter a ACL
privada da instalação. A CLI confirma pelo mesmo bloqueio de escritora de
`elzevir import`, com `elzevir serve` parado.

Em I7, `/equipe/acervo/importar` abre a lista dos lotes do catalogador e
oferece envio multipart em fluxo. A sessão fornece o operador e o servidor
fornece a instituição. O token CSRF precede o arquivo no formulário e é
conferido também nas ações de edição, prévia, confirmação e cancelamento.
A tela usa os leitores MARC da I6, mostra a detecção e edita a mesma
receita JSON estruturada. A troca de leitor invalida a revisão e passa
pela validação do arquivo inteiro. Evidência de planilha, SQL ou
contêiner impede a seleção de um leitor MARC. Revisão da origem e
paginação ficam na rota do lote.
O servidor chama a operação compartilhada com a CLI e usa a própria
escritora para confirmar, com o operador da sessão na auditoria.

Após confirmar, a resposta redireciona ao relatório com as contagens e
as posições de recusas. O operador pode baixar esse resultado como JSON.
Os dez relatórios mais recentes ficam na memória do servidor, cada um
acessível pelo mesmo catalogador até expirar dessa lista ou o servidor
reiniciar. Originais e receitas seguem as regras de remoção da I6.

A revisão inicial é 1. A receita JSON substitui a receita anterior e
contém `edits`, `exclude`, `accept_missing_barcodes` e `matching` opcionais.
Os números de registros começam em 1. `accept_missing_barcodes` aceita
exemplares sem código de barras nos registros indicados. Cada regra tem uma
etiqueta, um registro opcional e uma operação `change` com `op` e seus
parâmetros. O exemplo edita o título do primeiro registro usando `245$a`.

```json
{
  "edits": [{"record": 1, "tag": "245", "change": {"op": "replace_subfield", "code": "a", "value": "Título corrigido"}}]
}
```

As regras são aplicadas em ordem. A adição de campo põe o campo no fim.
Remoção, substituição e edição de subcampos ou texto atingem as ocorrências
selecionadas. `999 $c/$d` e `952 $9` de origem devem permanecer intactos.
A edição só publica a nova receita e exige uma nova prévia.

O manifesto vincula revisão, operador, instituição, formato e SHA-256 do
original. A prévia de lote verifica essa identidade, renderiza uma cópia
temporária limitada e registra a revisão como pronta se a validação
integral passar. A confirmação refaz essa validação antes de submeter
registros. ISO 2709 que não sofreu edição passa pela cópia byte a byte.
As amostras de revisão informam quais registros mudaram e exibem os campos
alterados antes e depois. Um manifesto persistido como confirmando bloqueia
reenvio após interrupção. O cancelamento desse estado exige
`--acknowledge-interrupted`.

O estado distingue preparando, em revisão, pronto, confirmando e encerrado.
Revisão do lote e identidade do original vinculam a confirmação à prévia
vista; alteração ou versão incompatível obriga nova revisão. Só uma
confirmação pode consumir o lote. Envio repetido, outra aba ou atualização
da página não submetem novamente os mesmos registros.

Confirmar é uma ação explícita, após o resumo de registros e exemplares,
avisos, casamento e substituições. Salvar edição nunca importa. A gravação
preserva a ordem e as transações por registro do pipeline; não promete uma
transação única do arquivo. Fila cheia aplica o controle de fluxo existente.
E/S ou escritora indisponível interrompe; confirmações anteriores ficam.

O resultado acrescenta contexto ao `ImportReport` existente, sem mudar seu
contrato: lidos, preparados, excluídos explicitamente, gravados, casados,
substituídos, ignorados, falhas e exemplares acrescentados/substituídos/
recusados. Falhas ficam em ordem de origem; posição e byte são usados quando
existem, linha/célula nos demais formatos. Contagens não escondem recusas de
exemplares nem apresentam sucesso total após interrupção.

Ao concluir a confirmação, inclusive com resultado parcial conhecido, ou
cancelar explicitamente, remove-se o original e as edições do lote. A tela
informa essa remoção e permite guardar o relatório, sem copiar dados da
origem. Cancelar pede confirmação; sair da tela não cancela. Lotes após
queda do processo permanecem identificados como interrompidos: não há
reenvio automático, pois pode ter ocorrido confirmação antes da queda.
Retomar a revisão ou cancelar exige conferir o resultado; não se promete
idempotência de banco nova. Falha de limpeza é informada e não apaga a
evidência de gravação nem permite confirmar de novo.

### 1.8 SQL, contêineres e privacidade

O leitor SQL aceita a gramática documentada e testada de `CREATE TABLE` e
`INSERT`, respeitando colunas declaradas, aspas e escapes. Não executa SQL,
scripts ou comandos de restauração. Extrai `biblio_metadata.metadata`,
associa `items` pelo `biblionumber` e acrescenta `952` pela borda e suas
correspondências. Não duplica exemplar já representado: conflito entre
inventário e MARC, chave duplicada ou exemplar órfão exige revisão.
Formato não coberto para; não adivinha posição de coluna ou relacionamento.

Tabelas `borrowers`, `borrower_*`, `issues`, `old_issues`, `reserves`,
`accountlines` e demais tabelas fora de catálogo/exemplares são ignoradas
sem materializar seus valores. Nenhum conteúdo delas entra em erro, log,
relatório ou perfil salvo. A presença isolada de tabela ignorada não prova
backup completo; o inventário de esquema completo é conferido antes de
oferecer importação parcial. Dúvida exige revisão, sem executar o dump.

Arquivos compactados são inspecionados antes de extrair. Cada membro recebe
a mesma detecção e limites; o inventário é revisado, não concatenado por
palpite. Planilha é reconhecida pela estrutura interna do pacote. Macros,
fórmulas executáveis e referências externas não são executadas. Arquivos
criados pelo usuário não são carregados como HTML nem buscados pela rede.
Logs contêm etapa, posição e código de problema, sem células ou registros.

### 1.9 Limites planejados

Constantes nomeadas, aplicadas durante leitura, antes de alocar ou expandir;
MiB/KiB são unidades binárias. Estes limites pertencem ao fluxo novo, não
reduzem os limites nem a capacidade do importador ISO direto existente.

| Constante | Valor e alcance |
|---|---|
| `MAX_IMPORT_FILE_BYTES` | 512 MiB por envio ou arquivo preparado, inclusive no CLI `--preview`. |
| `MAX_IMPORT_RECORDS` | 100.000 registros por lote preparado. |
| `MAX_PREVIEW_RECORDS` | 3 registros na primeira página; diagnóstico integral acessível. |
| `MAX_STAGED_BATCHES` | 10 lotes por instituição. |
| `MAX_STAGED_BATCH_BYTES` | 1 GiB por lote, incluindo original, edições e relatórios. |
| `MAX_STAGED_TOTAL_BYTES` | 4 GiB por instituição, reservados antes de aceitar trabalho concorrente. |
| `MAX_IMPORT_EDITS` | 10.000 operações por lote. |
| `MAX_BATCH_MANIFEST_BYTES` | 1 MiB de metadados e receita JSON por lote. |
| `MAX_IMPORT_PROFILE_BYTES` | 1 MiB por TOML de mapeamento. |
| `MAX_XML_DEPTH` | 32 níveis; DTD e entidades externas proibidas. |
| `MAX_XML_ELEMENT_BYTES` | 1 MiB de conteúdo direto acumulado por elemento; descendentes contam no registro. |
| `MAX_XML_RECORD_BYTES` | 1 MiB por registro XML. |
| `MAX_XML_ELEMENTS_PER_RECORD` | 20.000 elementos por registro. |
| `MAX_TABLE_ROWS` | 1.000.000 linhas lógicas no total, incluindo exemplares e tabelas. |
| `MAX_TABLE_COLUMNS` | 1.024 colunas por tabela. |
| `MAX_TABLE_CELL_BYTES` | 64 KiB por célula decodificada. |
| `MAX_TABLE_ROW_BYTES` | 1 MiB por linha lógica decodificada, inclusive CSV multilinha. |
| `MAX_MRK_LINE_BYTES` | 1 MiB por linha. |
| `MAX_MRK_RECORD_BYTES` | 1 MiB por registro textual. |
| `MAX_MRK_LINES_PER_RECORD` | 20.000 linhas por registro. |
| `MAX_MRK_INPUT_BYTES` | 512 MiB de entrada `.mrk`. |
| `MAX_MRK_RECORDS` | 100.000 registros por entrada `.mrk`. |
| `MAX_SQL_STATEMENT_BYTES` | 16 MiB por instrução, inclusive ao ignorar uma tabela. |
| `MAX_ARCHIVE_MEMBERS` | 1.000 membros, contando diretórios e pacotes de planilha. |
| `MAX_ARCHIVE_EXPANDED_BYTES` | 512 MiB no total expandido de um lote. |
| `MAX_ARCHIVE_RATIO` | 100:1 por membro e acumulado, conferidos também durante expansão. |
| `MAX_ARCHIVE_DEPTH` | 2 contêineres aninhados, contando o pacote de planilha. |

MARC convertido também obedece a `Limits` e à escrita ISO 2709 existentes
(99.999 bytes por registro, 5.000 campos). Descrições e valores BIBFRAME
obedecem aos limites e validadores atuais de [BIBFRAME completo](bibframe-completo.md).
Cada lote reserva espaço para duas versões do manifesto, para permitir
atualização atômica e cancelamento mesmo perto da cota.
Arquivos absolutos, caminhos com travessia, links, dispositivos e entradas
duplicadas que sobrescreveriam conteúdo são recusados na extração.
Limite atingido é erro explícito; nunca validação incompleta apresentada
como sucesso. Não se descartam lotes antigos silenciosamente para abrir espaço.

## 2. Origem no Koha

Referências já estabelecidas: `reference/koha/tools/stage-marc-import.pl`,
`tools/manage-marc-import.pl` e `C4/ImportBatch.pm` (incluindo
`_get_commit_action`), descritas em [casamento](casamento.md).
Não se portam tabelas de staging nem se executam os importadores do Koha.
I0 não realizou nova comparação contra uma instância de Koha.

Inspiração de interação: `PauloFBaldiFH/koha.nexus`, `docs/magic-import.md`,
`docs/field-mapping.md` e `panel/kei_panel/screens/fieldmap.py`.
Não se copiam conversão de planilhas para MARC, remoção de identificadores,
padrões de tipo de item, inferência de material por tipo ou fluxos de escrita.
Marva/LC pode inspirar a disposição visual dos blocos, sem copiar código.

## 3. Paridade

`livre` para detecção e interação. Exportação MARC continua na área de
paridade exigida. As provas existentes de importação, casamento, mapeamento,
exportação, criação, CLI e cobertura BIBFRAME permanecem sem alteração.

| PR | Entrega e prova específica, além da regressão completa |
|---|---|
| I0 | Apenas esta spec, ADR, contrato de interação e referências; revisão de Paulo. |
| I1 | Digest constante dos 200 registros com a configuração de `the_staff_export_imports_whole`; só teste. |
| I2 | Detector puro, fixtures sintéticas e recusa dos formatos ainda não suportados antes de gravar. |
| I3 | MARCXML com `quick-xml`, fuzzing, digest I1 e exportação byte a byte dos 200 registros convertidos no teste. |
| I4 | Leitor e importação de `.mrk`, escapes e fuzzing. Gerar a versão textual no teste e obter o digest I1 e a exportação ISO byte a byte. |
| I5 | Preparação integral e CLI `--preview`; digest antes/depois igual e erros equivalentes à confirmação. |
| I6 | Lote fora do banco e edição; sem edição mantém digest I1; editar um campo muda só esse campo na exportação. |
| I7 | Tela autenticada, permissão/CSRF, Fluent en/pt-BR, novos snapshots HTML e prova da função compartilhada com CLI. |
| I8 | CSV/TSV, fuzzing e modelo gerado. Vinte livros sintéticos, reimportação por ISBN sem duplicar, MARC válido, cadastros por nome, recusa transacional por exemplar e bloqueio de códigos de barras repetidos no lote. |
| I9 | SQL como dados, fuzzing e dump sintético gerado dos 200 registros, com digest I1 e tabelas privadas ignoradas. |
| I10 | `calamine` para `.xlsx`/`.ods`, ZIP/gzip e fuzzing; provas de I8 e recusa de bomba de compressão. |

Cada leitor novo tem alvo em `fuzz/fuzz_targets/`. Dependências aprovadas:
`quick-xml` (MIT), `csv` (MIT/Unlicense), `calamine` (MIT, só I10) e recurso
`multipart` de Axum. Nenhuma é adicionada em I0. Outra dependência exige
pergunta no PR antes de implementar.

Todo PR informa testes antes/depois, resumos de fmt, clippy, nextest, deny
e `xtask ci`, diff contra a base e ausência de mudanças nos arquivos
protegidos. A partir de I3 mede também o benchmark de importação; queda
superior a 10% ante a linha de base de [importação §4](importacao.md) exige
explicação e condições da medição. Fixtures novas são sintéticas; não se
alteram testes antigos, snapshots existentes ou o fixture dos 200 registros.

## 4. Casos de borda

1. Arquivo vazio ou só separadores: prévia zerada, sem comandos de gravação.
2. Extensão falsa, XML dentro de SQL, prefixo curto e detecção empatada:
   apresentar evidência; validação completa não aceita uma troca incompatível.
3. Registro truncado, XML com entidade externa, `.mrk` com escape desconhecido
   e célula acima do limite: erro localizado, sem correção adivinhada.
4. Dois cabeçalhos iguais, coluna reordenada ou perfil incompatível:
   identidade das colunas preservada e mapeamento incompleto bloqueado.
5. Código de barras ausente ou repetido, no arquivo e no catálogo:
   revisão explícita; nenhuma geração, substituição ou omissão automática.
6. Unidade desconhecida, nomes iguais em cadastros distintos e localização
   de outra unidade: resolver identidade pelo cadastro ou recusar.
7. Duas linhas com ISBN igual: continuam separadas sem opção de agrupamento;
   divergência de dados no grupo requer escolha do catalogador.
8. Mudança de formato, edição ou casamento após prévia: invalidar resultado
   antigo, inclusive se outra aba tentar confirmar sua revisão.
9. Estado do catálogo alterado durante a revisão: escritora decide novamente;
   a recusa real e a previsão aparecem separadas no relatório.
10. SQL com colunas fora de ordem, aspas, XML multilinha, itens órfãos,
    tabelas privadas ou esquema completo: respeitar gramática, revisão e recusa.
11. ZIP com travessia, link, nome duplicado, razão excessiva ou planilha
    aninhada: recusar antes de gravar além dos limites.
12. Queda, falta de espaço, limite de lotes, confirmação concorrente ou falha
    de limpeza: preservar estado conhecido e não reenviar registros cegamente.
13. `Esc`, navegação, sessão expirada ou erro de validação: nunca descartar
    edições silenciosamente nem transformar saída da tela em confirmação.

## 5. Desvios

Nenhum desvio novo em área de paridade. Melhorias de revisão não alteram
o casamento nem a exportação existentes. Se a solução exigir mudança de
comportamento de `import_marc21`, matching, mapeamento ou schema, parar e
perguntar no PR antes de fazê-la. Formato real não coberto ou dados que
pareçam pessoais também exigem interromper a análise e pedir orientação.

## 6. Fora do escopo

- Implementação em I0 e início de outro PR antes da mesclagem do anterior.
- Importação de autoridades (tarefa 2.9), leitores, circulação e reservas
  (Fase 3), senhas e restauração completa de banco Koha.
- Leitores próprios de Biblivre, Pergamum, ISIS e Biblioteca Fácil; UNIMARC,
  `.xls`, `.dbf` e gramáticas SQL não especificadas.
- Correção de ortografia, capitalização automática, IA e geração automática
  de códigos de barras ou padrões institucionais.
- Alterar comportamento de `marc_to_bibframe`/`bibframe_to_marc`, regras de
  `matching.rs`, `MarcReader`, schema, migrações, testes ou snapshots existentes.
- Alterar `creating.rs` e `editing.rs`; eventual helper público precisa de
  commit separado sem mudança de comportamento e revisão do alcance.
- Segunda escritora, transação global do lote, escrita pela prévia ou
  garantia nova de reexecução automática após resultado desconhecido.
