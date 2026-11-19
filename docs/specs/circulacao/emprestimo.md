# Empréstimo

> Status: tarefa 3.4, partes a (a decisão, pura, no domínio) e b (o comando `circ.checkout` e a tabela de empréstimos). Falta a gravação do oráculo. Base: Koha 26.05.05.

## 1. Comportamento

### Limite de empréstimos (`too_many`)

1. O limite vale para a unidade que controla o empréstimo, a categoria do leitor e a política do exemplar. Quem chama resolve a unidade (Koha: `CircControl`, padrão a unidade de origem do exemplar) e diz a unidade de cada empréstimo atual.
2. A regra `maxissueqty` (ou `maxonsiteissueqty`, no empréstimo local) que vale é a de `effective_rule` ([regras.md](regras.md), item 3). Contam contra ela:
   - os empréstimos da unidade da regra, quando ela nomeia uma;
   - se a regra nomeia uma política, só os empréstimos dessa política;
   - se a regra vale para qualquer política, os empréstimos das políticas que não têm regra `maxissueqty` própria na mesma unidade, para a categoria da regra ou a do leitor.
3. Depois, todos os empréstimos da unidade de controle contam contra `patron_maxissueqty` (e `patron_maxonsiteissueqty`), a regra por unidade e categoria.
4. A comparação (Koha: `_check_max_qty`):
   - o empréstimo local conta como normal quando `ConsiderOnSiteCheckoutsAsNormalCheckouts` está ligado (padrão), e então conta contra os dois limites;
   - sem limite local, só o limite normal vale; limite vazio é sem limite;
   - o valor da regra é lido como número do Perl: o número decimal do começo do texto, arredondado para cima, ou 0 quando não há;
   - passa do limite quem já tem tantos empréstimos quanto ele permite. Trocar um empréstimo normal por local (ou o contrário) do mesmo exemplar não soma um.
5. Sem nenhuma regra de limite que alcance o pedido, nada pode ser emprestado (Koha: `NO_RULE_DEFINED`, máximo 0).
6. O resultado diz o motivo (normal, local, sem regra), a contagem, o máximo como estava na regra e o escopo da regra.

### A verificação (`check_checkout`)

7. A verificação devolve três listas, cada uma na ordem do Koha: o que impede, o que pede confirmação e o que só avisa. O empréstimo é possível quando a primeira lista está vazia.
8. Ordem e efeito:
   - leitor com cadastro vencido: impede;
   - exemplar emprestado ao mesmo leitor: pede confirmação de renovação, e os limites não contam;
   - exemplar emprestado a outro: pede confirmação, ou só avisa que será devolvido quando `AutoReturnCheckedOutItems` está ligado (padrão desligado);
   - limite atingido: pede confirmação com `AllowTooManyOverride` ligado (padrão), senão impede. Com limite 0, pede antes a confirmação de que o leitor não pode emprestar;
   - exemplar não emprestável (`notforloan` diferente de 0): impede, ou pede confirmação com `AllowNotForLoanOverride` ligado (padrão desligado);
   - exemplar retirado (`withdrawn` maior que 0): impede;
   - exemplar restrito (`restricted` igual a 1): impede;
   - exemplar perdido: segundo `IssueLostItem`, pede confirmação, avisa (padrão) ou nada.
9. As preferências do Koha entram como `CheckoutSettings`, com os padrões do Koha.

### O período (`loan_rule`)

10. O período do empréstimo vem das regras que valem para a unidade, a categoria e a política, como no Koha:
    - `issuelength` ausente é 0 dias; `renewalperiod` ausente é 0, e vazio quer dizer "use `issuelength`";
    - `lengthunit` diferente de `hours` é dias;
    - `daysmode` vazio ou ausente cai na preferência `useDaysMode` (padrão `Calendar`);
    - `hardduedate` com `hardduedatecompare` ausente é 0: toda devolução cai na data fixa. Um `hardduedatecompare` fora de -1, 0 e 1 nunca se aplica.

### O comando (`circ.checkout`)

11. Pede a permissão de circulação. Recebe o leitor, o código de barras, a unidade do balcão, se é empréstimo local, se a equipe já confirmou e o momento, no fuso da biblioteca.
12. O código de barras é lido sem espaços nas pontas (Koha: `barcodedecode` sem filtro). Código desconhecido, leitor desconhecido ou unidade do balcão desconhecida são recusados.
13. A unidade de controle é a unidade em que o exemplar está (Koha: `CircControl` = `ItemHomeLibrary` com `HomeOrHoldingBranch` = `holdingbranch`, os padrões). Os empréstimos atuais do leitor contam pela unidade em que cada exemplar está. A data de devolução usa as regras e o calendário dessa unidade, sem horário de funcionamento e sem `ReturnBeforeExpiry` (os padrões), e é truncada no minuto.
14. O que impede recusa com a lista. O que pede confirmação recusa com a lista até a equipe confirmar. Os avisos voltam com o empréstimo.
15. Um exemplar já emprestado é recusado mesmo com confirmação: devolver e renovar chegam com a tarefa 3.5.
16. O empréstimo grava o leitor, o exemplar, a unidade do balcão, o momento, a data de devolução e se é local. O exemplar passa para a unidade do balcão e deixa de estar perdido (Koha: `AddIssue`).
17. Os empréstimos não entram no resumo da importação.

### Fora desta parte

18. Ficam para as tarefas que os trazem: dívidas (3.7), bloqueios, suspensões e atrasos (3.5), reservas (3.8). Cartão perdido e endereço inválido (`GNA`, `LOST`) chegam com o cadastro que os grava. O Elzevir não tem tipos de item pais, então a regra de limite do tipo pai do Koha não existe aqui.

## 2. Origem no Koha

- `C4::Circulation::CanBookBeIssued` (ordem das recusas, preferências).
- `C4::Circulation::TooMany` e `_check_max_qty`.
- `C4::Circulation::GetBranchBorrowerCircRule` (`patron_maxissueqty`).
- `C4::Circulation::AddIssue`, `GetLoanLength`, `GetHardDueDate`, `_GetCircControlBranch`; `Koha::CirculationRules::get_effective_daysmode`.
- Testes: `t/db_dependent/Circulation/TooMany.t`.

## 3. Paridade

`exigida`. Prova: os testes de `crates/elzevir-domain/src/checkout.rs` portam os casos de `TooMany.t` (sem regras; limites 0; local sem limite; empréstimo local enchendo o limite local; limite vazio; regra geral e específica; limite do leitor; números do Perl) e a ordem de `CanBookBeIssued`. `crates/elzevir-store/tests/checkout.rs` empresta pelo comando contra o banco: data de devolução com o calendário, renovação pedida, limite, nenhuma regra, recusas do leitor e do exemplar, exemplar perdido. Em seguida, as respostas do oráculo gravadas por `GET /api/v1/checkouts/availability` rodam contra o comando.

## 4. Casos de borda

1. Regra com texto `"4 books"`: vale 4. Texto `"many"`: vale 0, e nada pode ser emprestado.
2. Regra para qualquer política com limite 3 e regra própria da política de DVD: empréstimos de DVD não contam contra o 3.
3. Renovar com o limite atingido: não pede confirmação de limite.

## 5. Desvios

Nenhum.
