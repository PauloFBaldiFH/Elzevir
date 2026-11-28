# Importação universal de acervo, leitores e autoridades

Proposta de contrato após o estudo de 2026-11-28. O comportamento entregue até
o PR #304 está na [spec I0–I10](importacao-universal.md). A ordem de aprovação
de entregas consta no [doc 03](../../arquitetura/03-estrategia-reconstrucao.md).

## Unidade de trabalho e privacidade

O operador escolhe uma origem institucional estável e reúne uma ou mais
exportações **declaradas compatíveis**. Cada membro tem identificador de
lote, tamanho, SHA-256, caminho lógico, formato e conjunto. Nome, hash e data
do arquivo ajudam a identificar uma exportação. Outra exportação da mesma
instalação mantém o namespace. Cópias com o mesmo hash são reportadas antes
de agregar. Dois bancos de filiais com sequências locais independentes
precisam de namespaces distintos.

O inventário começa sem destinos selecionados. Distingue acervo (inclui
holdings e itens), leitores e autoridades. O operador pode selecionar qualquer
subconjunto não vazio das três classes. Movimentos são uma escolha adicional,
com permissão de circulação e referências resolvidas para leitor e item.
Conservar referências a classes excluídas como pendências tipadas quando a
operação selecionada depender delas. Recusar um vínculo inválido, inclusive
se já houver outro objeto com o mesmo nome ou número.

O servidor verifica permissão por classe, sessão, proprietário do lote,
revisão e CSRF em toda operação Staff. A CLI exige a mesma política de acesso
para dados pessoais e usa a mesma porta de aplicação. O acesso ao catálogo
não concede acesso à grade de leitores. Restringir originais mistos e staging
à equipe autorizada para as classes presentes, mesmo em prévia exclusivamente
bibliográfica. Mostrar contagens e localizações técnicas a quem tem acesso ao
inventário. Proibir credenciais, privilégios, tokens e PII em logs, receitas
exportáveis, RDF, busca pública e serviços de IA. Segredos incidentais ficam
restritos ao original privado durante sua retenção. Retenção, expiração,
permissões do diretório, recuperação e descarte devem ter testes antes de
usar cópia de uma biblioteca. O migrador dedicado Koha conserva seu contrato
operacional e suas permissões próprias.

## Evidência, plano e preparação

1. Detectar por assinatura, estrutura e validação completa dentro de limites.
   Separar transporte, dialeto, sistema/versão e papel por registro/tabela.
   Informar membros ignorados ou indecodificáveis, motivo e contagem conhecida.
   Preservar posição física, forma lexical, repetição, ordem, indicadores e
   charset. A planilha começa na primeira linha quando o cabeçalho é incerto.
2. Representar somente operações necessárias à receita aprovada. Começar com
   seleção por membro, coluna/caminho e repetição, chave simples/composta,
   junção e resolução de código, filtro, constante e mapeamento por papel.
   Ausente, nulo, vazio e inválido têm efeitos distintos. Conservar original e
   transformação em separado. O MVP pode percorrer novamente a fonte privada.
   Exigir medição e decisão de retenção antes de acrescentar staging em disco.
3. Oferecer propostas concorrentes com evidência inspecionável e contagens
   (pais distintos, referências não nulas, resolvidas, órfãs e ambíguas).
   Identificadores locais se qualificam por origem, entidade, sistema e
   valor lexical. Um ISBN, rótulo ou cartão repetido pede revisão. A inclusão
   de valores entre colunas sozinha é insuficiente para ligar. Conflitos de
   papel, titularidade, circulação ou origem impedem substituição.
4. Vincular o plano a digest de todos os membros, seleção, versão do leitor,
   versão da receita, cadastros escolhidos e revisão do lote. Invalidar a prévia
   se algum deles mudar. Usar a mesma preparação determinística em CLI e Staff.
   Uma sugestão eventual de IA passa pelas mesmas verificações e só altera uma
   proposta após revisão. A execução depende exclusivamente do plano aprovado.
5. A escritora revalida versão, candidatos, permissão, bibliotecas e dependências
   na transação da unidade afetada. Grava identidade, efeitos pendentes de
   reindexação e resultado durável junto à unidade. Persistir a posição de
   retomada vinculada a esse resultado. Se o processo cair depois do commit e
   antes do recibo, a repetição identifica o resultado anterior. A política de
   unidade deve preservar a atomicidade de uma Instância e seus itens aprovados.

## Regras por classe

- **Acervo.** Usar Obra/Instância/Item e projeções bibliográficas existentes.
  Distinguir holdings de item físico, tombo, número de cópia, código de barras,
  enumeração, cronologia, circulação e status de disponibilidade. Preservar
  campos locais ausentes, privacidade de notas, 245 repetível e origem MARC.
  Perguntar a política para unidade/coleção desconhecida e distinguir adição
  de edição. Nunca fabricar exemplar para preencher uma referência.
- **Autoridades.** Identificar por registro (inclusive líder/06 e família
  MARC21 ou UNIMARC). Separar forma autorizada, variantes, relações tipadas,
  qualificadores, subdivisões ordenadas, fontes, notas públicas/restritas,
  vocabulário, agência, $0 e $1. Registrar campos sem projeção no original.
  Usar Agente, Assunto e vínculos nativos. Permitir autoridade isolada e
  homônimos. Criar identidades antes de resolver relações recíprocas ou
  referências futuras. Acervo antes ou depois de autoridades deve convergir
  para a mesma ligação confirmada, preservando papel e origem. Autoria
  transcrita isolada só fornece uma menção.
- **Leitores e movimentos.** Aprovar categorias e unidades, máscaras e
  datas com convenção explícita, sem alterar zeros existentes. Identidade
  local, cartão e documento podem divergir. Funcionário, usuário de aplicação,
  autor e responsável intelectual têm papéis próprios. Importar cadastro
  mínimo e dependências autorizadas. Ignorar autenticação e privilégios.
  Movimentos históricos observam consentimento, retenção e privacidade.
  Abster-se em ocorrência sem leitor/item, evento indefinido ou exemplar já
  emprestado. Exigir prova independente de data/status e consistência do livro
  de circulação. A importação histórica não dispara avisos, multas ou reservas
  correntes por efeito colateral.

## Inventário PostgreSQL em fluxo (1a aprovado)

Para PostgreSQL textual, inventariar declarações e blocos de dados aceitos,
contando linhas até o terminador de COPY. Reconhecer identificadores com
schema, ordem das colunas e nulo lexical. Distinguir linha truncada,
quantidade errada de colunas, bloco incompleto e escapes. DDL e comandos
externos não são executáveis. Recusar COPY PROGRAM, arquivos externos e
outras opções sem gramática comprovada. Conteúdo que pareça um comando
dentro de um valor continua sendo dado. Manter limites cumulativos por
lote, membro, linha e valor. Reportar tabela vazia como presente e vazia.
O teste deve provar ausência de escrita inclusive com entrada malformada e
cancelamento, e comparar o inventário da CLI com o Staff autenticado.

A CLI local oferece `elzevir import inventory ARQUIVO`, com saída JSON. No
Staff, o formulário de inventário exige sessão com catalogação, leitores e
circulação, porque a classe do dump é desconhecida no upload. O arquivo é
lido uma vez. A resposta contém tabela, ordem das colunas de COPY e totais
de linhas, valores nulos e campos vazios. Não se devolvem valores. O
resultado é transitório, sem ID de lote nem ação de confirmação que possa
usar uma leitura antiga. Cancelar fecha o fluxo e descarta os totais.
Uma declaração sem COPY aparece com zero linhas e sem colunas de COPY.

O leitor aceita uma fonte textual de até 512 MiB com linhas físicas de até
4 MiB, campo de até 1 MiB, cabeçalho de até 64 KiB, 1.024 tabelas e 256
colunas por COPY. Aceita identificadores ASCII simples, inclusive entre
aspas e com schema. Valida contagem e ordem de colunas por bloco. Aceita
cabeçalhos UTF-8 e declaração explícita de UTF-8,
LF/CRLF, tabulação como delimitador e escapes textuais no conteúdo, incluindo
barra literal e tabulação codificada como `\t`. Literais SQL multilinha,
funções, layouts alterados, dados CSV/binários e continuações físicas exigem
outro leitor ou exportação. As declarações DDL são evidência estrutural. O
inventário desconsidera metadados auxiliares reconhecidos do pg_dump.

## Conjunto privado de fontes (1b)

A equipe seleciona de 1 a 64 arquivos locais da mesma exportação compatível
e informa a origem institucional estável. Na CLI, usar `elzevir import
stage-set --data-dir DIR --operator ID --origin ORIGEM ARQUIVO...`. Na tela
de importação, usar o formulário de conjunto de fontes com as permissões de
catalogação, leitores e circulação. Esses privilégios e a titularidade também
se aplicam à listagem, revisão e remoção. Lotes de um único arquivo criados
pelo fluxo anterior mantêm o contrato original.

O manifesto conserva origem, posição (a partir de 1), nome lógico, tamanho,
posição inicial em bytes, SHA-256 e hipótese de formato pelo prefixo de cada
membro. A identidade local do membro é lote mais posição. Nomes repetidos
continuam distintos. Conteúdo idêntico aponta para a primeira ocorrência.
Para selecionar membros de ZIP ou gzip, preparar primeiro o contêiner com
`elzevir import stage`. No inventário, contar as entradas a partir de 1.
Por exemplo, `elzevir import archive-set --data-dir DIR --operator ID
--origin ORIGEM LOTE REVISAO 3 1` cria outro lote com os membros 3 e 1,
nessa ordem. No Staff, abrir o rascunho do contêiner, informar a origem e
selecionar os membros desejados. A revisão, a titularidade e o SHA-256
do original são conferidos antes da extração. O leitor valida o contêiner
inteiro, inclusive membros fora da seleção e nomes em contêineres aninhados.
Posições repetidas ou inexistentes invalidam a operação inteira.

Para cada membro extraído, o manifesto registra também o caminho exato em
cada nível do contêiner. O SHA-256 do contêiner vincula a seleção à fonte.
O novo lote contém os bytes dos membros e pode ser reaberto depois do
cancelamento do lote do contêiner. Uma planilha selecionada dentro de um ZIP
é preservada com todas as abas. A escolha de uma única aba para o catálogo
continua disponível no formulário de mapeamento.

A captura é privada e integral, sob a trava e as cotas existentes. O nome
informado nunca vira caminho de armazenamento. Limitar o conjunto a 512 MiB
no total. Uma falha de leitura ou envio incompleto descarta a captura. A
ação de verificar na prévia relê os bytes e confere os digests, os limites
dos membros e o fim exato do original. O conjunto fica disponível para revisão e cancelamento. A etapa
de descoberta não autoriza edição ou confirmação no catálogo. Vínculos
entre arquivos podem ser conferidos na [prévia de relações da 1c](relacoes-entre-fontes.md).

## Integração com as autoridades nativas

Em `main` a0be08ba, `elzevir-app/src/authorities.rs` fornece
`AuthorityReader`, `AgentView`, `SubjectView` e validação de vínculo por
papel. `elzevir-store/src/authorities.rs` aplica edição, vínculo, versão
e alcance. O leitor Turtle em `elzevir-app/src/rdf/parse.rs` e a leitura
de componentes MADS em `rdf/read.rs` atendem ao intercâmbio de descrições
bibliográficas. O conversor bibliográfico UNIMARC também tem escopo próprio.
O leitor Turtle existente reorganiza grafos em árvores, recusa nós anônimos
compartilhados e projeta texto/idioma sem guardar o datatype no literal
projetado. Reusar esse resultado como original apagaria informação. Para
autoridades, qualificar Turtle e RDF/XML separadamente com oráculos de
triplas, listas MADS ordenadas e grafos com referências compartilhadas.
A projeção de autoridade independente precisa de um contrato de escrita no
módulo nativo em desenvolvimento pelo Claude. Referências de integração:
[autoridades](autoridades.md) e [modelo](bibframe-completo.md).

| Entrada futura | Evidência que deve sobreviver à projeção |
|---|---|
| MARC21 Autoridades | Agência e controle 003/001, 1XX, 4XX, 5XX com subcampos de controle, subdivisões, qualificadores, fontes 670, notas 667/68X e identificadores com seus sistemas. |
| BIBFRAME/RDF | IRI e tipo de Agente/Assunto, identificadores tipados, linguagem dos literais, papéis, relações dirigidas, origem e proveniência administrativa. |
| MADS/RDF | Forma autorizada e variantes com tipo, listas ordenadas de elementos/componentes, esquema, relações hierárquicas, associativas e equivalências com a força semântica original. |
| UNIMARC Autoridades | Selecionar por família e tipo do registro, aplicar definições próprias de 2XX/4XX/5XX e notas, conservar indicadores, subcampos, repetição e charset. |

Antes da primeira escrita de autoridades, conferir a revisão corrente da
main e as specs e comandos entregues pelo Claude. A ligação interativa por nome e papel exige uma Obra,
enquanto o importador precisará admitir Agente ou Assunto isolado e evitar
mescla de homônimos pela forma textual. Pactuar com o mantenedor
quais contratos nativos oferecem identidade independente, variantes,
qualificadores, procedência, notas com visibilidade e vínculos entre
autoridades. Onde faltar cobertura, manter a unidade pendente com o original
privado e durável (bytes, formato, charset/base IRI, origem, posição e
digest). O descarte de um lote temporário não pode apagar a única evidência
de uma projeção parcial confirmada. Registrar campos/triplas sem destino e
versão do mapeamento para reprocessamento idempotente. Preservar escopo de
nós anônimos por documento, etiquetas de idioma e tipos dos literais. Aceite
exige comparar original, projeção e vínculos após evolução do módulo, com
homônimos, variantes e relações recíprocas, sem reidentificar pelo rótulo.
BIBFRAME continua o modelo interno. Nenhuma tabela paralela de autoridade
MARC é necessária para inventariar fontes ou avançar os leitores independentes.

## Contabilidade e oráculos

Por classe e fonte: lidos = reconhecidos + não interpretados. Entre os
reconhecidos, distinguir selecionados e excluídos. Cada unidade selecionada
recebe exatamente um resultado final entre criada, atualizada, vinculada,
inalterada, ignorada por decisão, pendente ou rejeitada. Registrar relações
e efeitos em contadores próprios. Divisões e agrupamentos têm chave e
cardinalidade explícitas. Reapresentar arquivo e receita deve preservar
identidades após confirmação. Erro no último membro de uma unidade invalida a
unidade inteira. Recibos parciais identificam unidades já confirmadas.

Oráculo da fixture PostgreSQL sintética em base vazia. As colunas contam
descrições, itens, leitores e autoridades esperados **após implementação**.
O plano de teste usa uma Instância por descrição e conserva os dois homônimos.

| Seleção | Descrições | Itens | Leitores | Autoridades |
|---|---:|---:|---:|---:|
| Acervo | 2 | 3 | 0 | 0 |
| Leitores | 0 | 0 | 2 | 0 |
| Autoridades | 0 | 0 | 0 | 2 |
| Acervo e leitores | 2 | 3 | 2 | 0 |
| Acervo e autoridades | 2 | 3 | 0 | 2 |
| Leitores e autoridades | 0 | 0 | 2 | 2 |
| Três classes | 2 | 3 | 2 | 2 |

Com acervo e autoridades, a contribuição `01 → A1 (aut)` pode ser resolvida.
`02 → A9 (ill)` continua pendente. Com movimentos selecionados e dependências
presentes, M1 é aberto, M2 devolvido, M3 pendente por falta do leitor 99.
A tabela de operadores tem zero destinos em todas as seleções. Os números 01
e 02 aparecem em tipos distintos de propósito. O XML independente acrescenta
uma variante, uma fonte e duas referências recíprocas entre A1 e A2. A
fixture e o teste de recusa bibliográfica já podem ser executados. A tabela
acima é critério para as futuras provas no writer, CLI e Staff.
