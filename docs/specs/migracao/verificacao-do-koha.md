# Verificação da migração do Koha (v0)

> Status: v0, tarefa 6.2 do [doc 03](../../arquitetura/03-estrategia-reconstrucao.md) (Fase 6, aprovada por Paulo em 2026-11-27). Itens 1 a 12 na 6.2a (unidades, categorias, leitores, registros e exemplares, PR #277); itens 13 a 17 na 6.2b (empréstimos, reservas, contas e histórico, PR #278); item 18 na 6.3a (regras de circulação); item 19 na 6.3b (calendário). Base: [troca do Koha](troca-do-koha.md), [leitores](leitores-do-koha.md) e [circulação](circulacao-do-koha.md) do Koha. Critério: **zero diferença sem explicação, sem dado pessoal no relatório**.

A troca e a migração escrevem um relatório do que deixaram de fora. Esse relatório conta o que o próprio migrador decidiu. O verificador faz outra coisa: lê o dump outra vez e o banco do Elzevir por conta própria, compara os dois pelo número do Koha e explica cada diferença pelo que vê no dump e no banco, sem usar o relatório da migração. Uma diferença que ele não consegue explicar é um defeito a investigar.

## 1. Comportamento

### O comando

1. `elzevir verify-koha --data-dir DIR DUMP` lê o mesmo dump da troca (inteiro, com ou sem gzip) e o banco da pasta, que fica travada durante a leitura, como em todo comando que abre a pasta. Não grava nada.
2. O dump é lido duas vezes: pela leitura da migração dos leitores (o mesmo plano, com as mesmas regras de limpeza) e pela leitura do acervo da troca. O banco é lido pelas tabelas de borda que guardam os números do Koha (`koha_codes`, `koha_keys`, `import_identities`, `item_origins`).
3. **Saída.** O relatório vai para a saída padrão. O comando termina com erro quando há alguma diferença sem explicação, para que um roteiro possa parar nele.

### O que é comparado

4. **Unidades.** Cada `branchcode` de `branches` precisa estar no Elzevir, com o mesmo nome (`branchname`).
5. **Categorias.** Cada `categorycode` de `categories` precisa estar no Elzevir.
6. **Leitores.** Cada `borrowernumber` de `borrowers` precisa estar no Elzevir uma vez, com o mesmo número do cartão, a mesma unidade e a mesma categoria. A comparação usa os valores depois da limpeza da migração (itens 9 e 10 de [leitores-do-koha.md](leitores-do-koha.md)). O leitor anonimizado no Elzevir depois da troca só é contado.
7. **Registros.** Cada `biblionumber` de `biblio_metadata` precisa estar no Elzevir em exatamente uma Instância.
8. **Exemplares.** Cada `itemnumber` de `items` precisa estar no Elzevir uma vez, na Instância do seu `biblionumber`, com o mesmo código de barras, a mesma unidade de origem e a mesma unidade atual. Quando o exemplar tem tipo (`itype`), o tipo também precisa ser o mesmo.
9. **O que sobra.** Um número do Koha que o Elzevir tem e o dump não tem é uma diferença sem explicação.

### As explicações

10. Uma diferença tem explicação quando o verificador encontra a causa no dump ou no banco:
    - **Leitor fora:** o motivo da limpeza (sem número, sem cartão, sem nome, unidade ou categoria que não existe), como a migração o decide pelas mesmas regras.
    - **Registro fora:** um exemplar dele com unidade que não está em `branches` ou que não chegou ao Elzevir, porque a importação recusa o registro inteiro ([troca-do-koha.md](troca-do-koha.md), caso de borda 2).
    - **Registro ilegível:** a troca deixou o registro de fora porque o MARCXML dele não pode ser lido ([troca-do-koha.md](troca-do-koha.md), caso de borda 4); os exemplares dele ficam fora como "registro fora".
    - **Exemplar fora:** o registro dele está fora, ou o código de barras dele se repete no dump.

    Qualquer outra diferença, incluindo um valor diferente num leitor ou num exemplar que chegou, fica sem explicação.

### O relatório

11. Para cada tabela, o relatório dá quantos números o dump tem, quantos chegaram, quantos ficaram fora com explicação e quantas diferenças ficaram sem explicação. Depois vêm as explicações, agrupadas por causa, com os números do Koha, e por fim cada diferença sem explicação, com o número do Koha e o nome do campo (`cardnumber`, `barcode`, `homebranch`...).
12. **Sem dado pessoal.** O relatório nunca traz um valor: nem nome, nem cartão, nem código de barras, nem endereço. Só os números do Koha, os códigos das unidades e dos tipos e os nomes dos campos.

### Circulação (6.2b)

13. **Empréstimos**, por `issue_id`: o exemplar do empréstimo, achado pelo `itemnumber`, emprestado no Elzevir ao mesmo leitor, com o mesmo vencimento (`date_due`). Um empréstimo do Elzevir num exemplar que nenhum empréstimo do dump tem é nomeado pelo `itemnumber`, porque o Elzevir não guarda o `issue_id` de um empréstimo em curso.
14. **Reservas**, por `reserve_id`: uma reserva do mesmo leitor na Instância do mesmo `biblionumber`, na mesma situação (`found`: fila, separada ou em trânsito), com a mesma suspensão (`suspend` e o dia de `suspend_until`); e, em cada registro, as reservas da fila que chegaram na mesma ordem do Koha (`priority`, depois `reserve_id`). Uma reserva do Elzevir que não corresponde a nenhuma do dump é nomeada pelo `biblionumber`.
15. **Contas**, por `borrowernumber`: o saldo em centavos, como o Koha o calcula pela soma de `amountoutstanding` das linhas, e o número de linhas.
16. **Histórico**, por `issue_id` de `old_issues`: o empréstimo devolvido no Elzevir uma vez, com o leitor só quando ele chegou e guarda o histórico de leitura (`privacy`), e sem o leitor nos outros casos. O leitor anonimizado depois da troca não é comparado.
17. **Explicações da circulação**, além das do item 10: o motivo pelo qual a migração deixa a linha de fora pelo que falta no dump (sem número, leitor, exemplar ou data); o leitor, o exemplar ou o registro que não chegou; a unidade que o Elzevir não tem; o exemplar que chegou emprestado por outro empréstimo do dump; a reserva repetida do mesmo leitor no mesmo registro.

### Regras de circulação (6.3a)

18. **Regras**, por `id` de `circulation_rules`: uma regra do Elzevir com o mesmo nome, no escopo dos mesmos códigos do Koha (unidade, categoria e tipo de item, ou "qualquer"), com o mesmo valor. A regra que a troca não aceita (item 7a de [troca-do-koha.md](troca-do-koha.md)) é explicada pelo mesmo motivo, e a regra com unidade, categoria ou tipo que o Elzevir não tem é explicada pelo código. Uma regra do Elzevir que o dump não tem é nomeada pelo tipo e pelo escopo, com `*` para "qualquer" e `#` e o número para um cadastro do Elzevir sem código do Koha.

### Calendário (6.3b)

19. **Calendário**, por `id` de `repeatable_holidays` e de `special_holidays`: no calendário da unidade de mesmo `branchcode`, uma entrada no mesmo dia (dia da semana, dia e mês, ou data), com o mesmo título e a mesma descrição, como a troca os dá (item 7b de [troca-do-koha.md](troca-do-koha.md)), e, numa data, a mesma situação (fechada ou aberta, `isexception`). A linha que a troca deixa de fora é explicada pelo mesmo motivo, e a linha de uma unidade que o Elzevir não tem é explicada pelo código. Uma entrada do Elzevir que o dump não tem é nomeada pelo código da unidade e pelo dia (`CTR weekday 0`, `CTR annual 12-25`, `CTR 2026-12-24`).

## 2. Casos de borda

1. Dump sem `borrowers` ou sem `biblio_metadata`: recusado, como na troca.
2. Pasta que nunca recebeu a troca: cada número do dump vira uma diferença sem explicação, e o comando termina com erro.
3. Dois exemplares do dump com o mesmo código de barras: o que chegou é comparado; o outro fica fora com explicação.
4. O verificador roda logo depois da troca. Um empréstimo, uma reserva ou um pagamento feito no Elzevir depois dela aparece como diferença sem explicação, porque o dump não o tem.

## 3. Origem no Koha

- As tabelas `branches`, `categories`, `borrowers`, `biblio_metadata`, `items`, `issues`, `reserves`, `accountlines`, `old_issues`, `circulation_rules`, `repeatable_holidays` e `special_holidays` de `installer/data/mysql/kohastructure.sql`, lidas pelos mesmos leitores da migração.
- O saldo do leitor como `Koha::Account::balance` o calcula: a soma de `amountoutstanding`.

## 4. Paridade

`livre` (migração). Prova:
- testes de unidade da comparação em `elzevir-app::koha_verify`: cada explicação, cada campo diferente (cartão, código de barras, vencimento, saldo, ordem da fila, leitor no histórico, valor da regra, título e exceção do calendário), número que sobra, leitor anonimizado;
- teste do binário (`crates/elzevir/tests/cli.rs`): depois da troca do dump sintético, o verificador termina sem diferença sem explicação e com o registro de unidade desconhecida explicado; numa pasta sem a troca, termina com erro;
- o ensaio da [troca](troca-do-koha.md#5-roteiro-do-ensaio) roda o verificador depois da troca, e Paulo traz o relatório, que não tem dado pessoal.
