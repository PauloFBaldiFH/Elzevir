# Importação universal: preparação, revisão e confirmação

> Status: implementação incremental I0–I10; I2 acrescenta detecção à CLI,
> preservando ISO 2709. Novos leitores, prévia e tela entram nas etapas abaixo.
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

O mapeamento é TOML versionado: identidade da coluna, destino BIBFRAME
qualificado pela entidade ou campo de exemplar, e transformações declaradas.
Colunas com cabeçalhos iguais são identificadas separadamente. A versão é
obrigatória; versão desconhecida e destino inválido bloqueiam a preparação.
Sinônimos de cabeçalho vivem em arquivo de dados. Cabeçalhos, distribuições,
anos e ISBN com dígito verificador só sugerem destinos; o catalogador aprova
o que for incerto. Coluna não usada é excluída por escolha visível.

Unidade de origem e atual, localização, coleção e política são resolvidas
por **nome** nos cadastros genéricos. Localização e coleção pertencem à
unidade escolhida. Nome ausente, desconhecido ou ambíguo exige revisão;
nunca cria código, tipo de item ou padrão institucional. Classes, conteúdo,
mídia e suporte vêm das listas oficiais da LC, sem inferência por política.
Códigos e `koha_type_statements` permanecem exclusivamente na borda MARC.

O modelo BIBFRAME para baixar tem cabeçalhos derivados da ontologia
vendorizada em `reference/bibframe`, com entidade para distinguir propriedades
homônimas. Células estruturadas ou repetidas usam uma lista JSON dos valores
`Value` existentes (literal, conceito, IRI, entidade ou recurso), sem achatar
o grafo. Texto simples só vale quando o destino admite interpretação única;
relações exigem identidade resolvida. A versão do modelo e a ontologia são
identificáveis. O modelo válido entra sem mapear colunas manualmente;
propriedades e valores são validados como na criação.

Cada linha descreve uma Instância com seus exemplares. Agrupar linhas por
ISBN/título só ocorre após escolha explícita, exibindo grupos e conflitos.
ISBN ou título coincidente não funde Obras silenciosamente. Mapeamento salvo
contém regras e cabeçalhos, nunca linhas, amostras ou valores extraídos do
arquivo. Reaplicá-lo exige conferir colunas ausentes ou incompatíveis.

**Dependência antes de I8:** hoje `NewRecord.items` é `Vec<ItemData>`, com
códigos MARC, enquanto `ItemFields` usa identificadores dos cadastros nativos.
Não existe nessa porta uma entrada equivalente para exemplares nativos.
Essa diferença exige uma decisão revisada antes de implementar planilhas
com exemplares. Não se resolve criando códigos artificiais, passando por
MARC, usando `SaveItems` depois de importar ou abrindo outra escritora.
I0 não autoriza alteração desse contrato nem dos arquivos protegidos.

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

`elzevir import --data-dir DIR --preview ARQUIVO` é a interface planejada
para preparar sem gravar catálogo, auditoria, códigos ou efeitos. A tela
chama a mesma função de aplicação. Consulta ao catálogo usa só o pool de
leitura, por portas de `elzevir-app`, inclusive para cadastros e colisões.
`--preview` termina com saída 1 se houver erro ou revisão obrigatória pendente;
prévia válida, inclusive com avisos, termina com 0. O ISO direto não muda.

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
| `MAX_SQL_STATEMENT_BYTES` | 16 MiB por instrução, inclusive ao ignorar uma tabela. |
| `MAX_ARCHIVE_MEMBERS` | 1.000 membros, contando diretórios e pacotes de planilha. |
| `MAX_ARCHIVE_EXPANDED_BYTES` | 512 MiB no total expandido de um lote. |
| `MAX_ARCHIVE_RATIO` | 100:1 por membro e acumulado, conferidos também durante expansão. |
| `MAX_ARCHIVE_DEPTH` | 2 contêineres aninhados, contando o pacote de planilha. |

MARC convertido também obedece a `Limits` e à escrita ISO 2709 existentes
(99.999 bytes por registro, 5.000 campos). Descrições e valores BIBFRAME
obedecem aos limites e validadores atuais de [BIBFRAME completo](bibframe-completo.md).
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
| I4 | `.mrk`, escapes e fuzzing; gerar a versão textual no teste e obter digest I1. |
| I5 | Preparação integral e CLI `--preview`; digest antes/depois igual e erros equivalentes à confirmação. |
| I6 | Lote fora do banco e edição; sem edição mantém digest I1; editar um campo muda só esse campo na exportação. |
| I7 | Tela autenticada, permissão/CSRF, Fluent en/pt-BR, novos snapshots HTML e prova da função compartilhada com CLI. |
| I8 | CSV/TSV, fuzzing e modelo gerado; 20 livros sintéticos, reimportação ISBN sem duplicar, MARC válido e cadastros por nome. |
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
