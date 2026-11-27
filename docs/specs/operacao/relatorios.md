# Relatórios fixos

> Status: tarefa 5.7. Base: Koha 26.05.05, [doc 03](../../arquitetura/03-estrategia-reconstrucao.md) (tabela da Fase 5, linha 5.7): relatórios prontos, não SQL livre.

## 1. Comportamento

### Circulação

1. Um período tem o primeiro e o último dia, os dois incluídos; o que termina antes de começar é recusado. Um momento guardado traz o fuso da biblioteca, então o dia dele é o da biblioteca.
2. **Categoria do empréstimo.** O empréstimo aberto conta na categoria atual do leitor. O encerrado conta na categoria que o leitor tinha na devolução, guardada no próprio empréstimo (`returned_loans.category_id`, migração 68). Ela fica lá mesmo quando o leitor não guarda histórico, é anonimizado ou sai, como na tabela `statistics` do Koha. A migração preenche os empréstimos encerrados que ainda tinham leitor. A migração do Koha guarda a categoria do leitor migrado. Ao excluir uma categoria, os empréstimos encerrados dela passam a contar em "sem categoria".
3. **Circulação por unidade e categoria:** para cada unidade e categoria, os empréstimos feitos no período, na unidade do balcão que emprestou, e as devoluções feitas no período, na unidade que recebeu. As linhas vêm na ordem do nome da unidade e da categoria ("sem categoria" por último), seguidas de uma linha de total.
4. Empréstimo e devolução no mesmo período contam cada um no seu lugar.

### Acervo

5. **Acervo por classe e conteúdo:** os exemplares não baixados, pela classe principal da CDD e pelo tipo de conteúdo. A classe principal é a centena (000 a 900) da primeira linha da lombada (a divisão de [etiquetas.md](../catalogo/etiquetas.md), item 4) que começa com três algarismos ou mais: `869.3 A474d` é 800–899, `J 028.5 L732m` é 000–099. Número de chamada sem classe vai para "sem classe".
6. O conteúdo é o do ícone da Instância (Obra e Instância, [bibframe-completo.md](../catalogo/bibframe-completo.md), item 15); sem descrição de conteúdo, "não descrito".
7. As linhas vêm por classe (sem classe por último) e, dentro dela, na ordem dos tipos de conteúdo, seguidas do total.

### Leitores

8. **Leitores ativos:** para cada unidade de origem e categoria, os leitores (sem os anonimizados) e, deles, os ativos: os que pegaram empréstimo no dia dado ou depois dele.
9. Cada leitor guarda o dia do seu último empréstimo (`patrons.last_issued_on`, migração 68, como o `borrowers.lastseen` do Koha marca um empréstimo). Esse dia é atualizado em cada empréstimo, no balcão, no autoatendimento e na migração do Koha. A migração preenche esse dia com os empréstimos que ainda tinham leitor. "Ativo desde um dia" é o último empréstimo nesse dia ou depois dele, por isso a conta é exata sem guardar o histórico.

### Tela

10. **Relatórios** em `/equipe/relatorios`, para qualquer pessoa da equipe com sessão: os relatórios só têm contagens, nenhum dado de leitor. O link fica no módulo Relatórios da barra.
11. A página tem um formulário por relatório: a circulação pede o período (por padrão, do primeiro dia do mês até hoje), o acervo não pede nada e os leitores pedem o dia inicial (por padrão, um ano antes de hoje). O relatório pedido aparece em tabela embaixo dos formulários.
12. Cada tabela tem o link "Baixar em CSV" com o mesmo relatório: `relatorio-circulacao.csv`, `relatorio-acervo.csv` ou `relatorio-leitores.csv`, com cabeçalho e textos na língua da página, a linha de total e as mesmas regras de célula do CSV do inventário (aspas e `'` antes do que a planilha leria como fórmula).
13. Dia inválido, período invertido ou relatório desconhecido voltam à página com a mensagem (422).

## 2. Origem no Koha

- `reports/issues_stats.pl` (circulação por unidade e categoria num período), `reports/catalogue_stats.pl` e `reports/catalog_by_itemtype.pl` (acervo por número de chamada e tipo) e `reports/borrowers_stats.pl` (leitores por categoria e unidade, ativos com empréstimo desde uma data).
- A tabela `statistics` (categoria guardada em cada movimento) e `borrowers.lastseen`.

## 3. Paridade

`livre` (relatórios não estão entre as áreas de paridade exigida). As provas:

- **Unidade:** testes em `elzevir-domain::labels` (classe principal) e em `elzevir-app::reports` (período).
- **Loja:** `crates/elzevir-store/tests/checkout.rs` empresta e devolve de verdade e confere cada relatório com a contagem direta nas tabelas (empréstimos do mês, devoluções do mês, exemplares não baixados). Confere também que a devolução sem histórico continua na categoria e os ativos desde abril e desde março.
- **Servidor:** `crates/elzevir/tests/patrons.rs` confere a página, o link na barra, os leitores, o CSV do acervo, o total da circulação e a recusa do período invertido.

## 4. Casos de borda

1. Leitor que não guarda histórico devolve: o empréstimo encerrado perde o leitor, mas continua contando na categoria e na unidade.
2. Período sem movimento: só a linha de total, com zeros.
3. Exemplar sem número de chamada: "sem classe".

## 5. Desvios

- O `borrowers_stats.pl` do Koha conta como ativo só quem tem empréstimo encerrado (`old_issues`) desde a data; o Elzevir conta também o empréstimo em aberto.
- O Koha monta os relatórios com filtros e cruzamentos livres (linha × coluna); o Elzevir tem estes três relatórios fixos. Relatório com SQL livre não entra.
