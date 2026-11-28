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

## Primeiro incremento proposto

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
