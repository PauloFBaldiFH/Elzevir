# Leitores e categorias de leitor

> Status: tarefa 3.1 completa: partes a (modelo e comandos), b (busca, ficha e categorias na tela) e c (cadastro e edição pela tela). Base: Koha 26.05.05, [doc 03](../../arquitetura/03-estrategia-reconstrucao.md) (tabela da Fase 3, linha 3.1) e [interface](../../UI_BIBFRAME_INTERACTION_SPEC.md) §18.4.

## 1. Comportamento

### Categorias de leitor

1. A categoria é um cadastro genérico criado pela biblioteca, como unidades e políticas de circulação: nome legível (1 a 256 caracteres, único) e período de inscrição. Não há categoria nem sigla de fábrica.
2. O período de inscrição é **um** de dois: um número de meses (1 a 999) ou uma data fixa de validade.
3. Renomear e mudar o período não muda a validade de quem já está inscrito. Apagar uma categoria que algum leitor usa é recusado.

### Leitor

4. Campos do leitor:

   | Campo | Regra |
   |---|---|
   | Carteirinha | obrigatória, única, até 32 caracteres, sem espaço nas pontas |
   | Sobrenome | obrigatório, até 256 caracteres |
   | Nome | opcional, até 256 caracteres |
   | CPF | opcional, único; aceita pontuação e guarda só os 11 algarismos; os dois dígitos verificadores são conferidos, e 11 algarismos iguais são recusados |
   | Nascimento | opcional; não pode ser depois da inscrição |
   | E-mail | opcional, até 254 caracteres, com um `@` que não está na ponta |
   | Telefone | opcional, até 50 caracteres |
   | Endereço | opcional, até 512 caracteres |
   | Unidade | obrigatória, do cadastro de unidades |
   | Categoria | obrigatória, do cadastro de categorias |
   | Inscrição | data; na criação, o dia de hoje no fuso da biblioteca |
   | Validade | data; na criação, calculada pela categoria quando não vem preenchida |
   | Observação interna | opcional, até 1.000 caracteres, só para a equipe |
   | Histórico de leitura | guardar ou não guardar; o padrão é **não guardar** (interface §18.4) |

5. Validade calculada pela categoria: com período em meses, a inscrição mais os meses, limitada ao último dia do mês quando o dia não existe (31/01 + 1 mês = 28/02 ou 29/02); com data fixa, a própria data.
6. O leitor está **vencido** quando a validade é anterior ao dia de hoje no fuso da biblioteca.
7. Criar devolve o leitor; editar troca todos os campos de uma vez e exige o `row_version` lido (edição concorrente é recusada). Cada criação e edição vai para o `audit_log` com quem fez.
8. O CPF é mostrado mascarado (`***.456.789-**`) em toda tela; o inteiro só aparece no campo de edição, para quem tem a permissão de cadastro de leitores.
9. Permissões novas: **circulação** (Koha: `circulate`), que vai abrir a ficha e emprestar, e **leitores** (Koha: `borrowers`), que cria e edita leitores e categorias.

### Na tela (parte b)

10. **Busca** em `/equipe/leitores?q=`, para quem tem a permissão de circulação ou a de leitores. O texto acha a carteirinha inteira, o CPF (com ou sem pontuação) e, com duas letras ou mais, um pedaço de "sobrenome nome" ou "nome sobrenome", sem diferença entre maiúscula e minúscula. `%` e `_` são letras, não curingas. Até 50 leitores, por sobrenome e nome. Cada linha mostra carteirinha, nome, categoria e validade, com "vencido" quando for o caso.
11. Uma busca que acha **um só leitor, pela carteirinha**, vai direto à ficha (o bipe da carteirinha).
12. **Ficha** em `/equipe/leitores/{id}`, do outro lado do muro (interface §18.1): só com a permissão de circulação. Cada abertura vai para o `audit_log` (`patrons.view`, com quem abriu e quando) **antes** de os dados saírem do servidor. A ficha mostra os campos preenchidos, o CPF mascarado e um aviso quando a inscrição venceu.
13. **Categorias** em `/equipe/leitores/categorias`, para quem tem a permissão de leitores: nome e período (meses **ou** data fixa; os dois, ou nenhum, são recusados), quantos leitores cada uma tem, criar, salvar e apagar. As recusas aparecem na própria página.
14. A página inicial da equipe só mostra o link "Leitores" para quem tem a permissão de circulação ou a de leitores. Nenhuma página do acervo traz dado de leitor.

### Cadastro e edição (parte c)

15. **Novo leitor** em `/equipe/leitores/novo` e **edição** em `/equipe/leitores/{id}/editar`, só com a permissão de leitores; a busca mostra o link "Novo leitor" e a ficha mostra "Editar leitor" para quem tem essa permissão. Unidade e categoria são listas de seleção dos cadastros; sem nenhuma categoria, o formulário avisa que é preciso criar uma antes.
16. No leitor novo, a inscrição é o dia de hoje no fuso da biblioteca e a validade vazia vem da categoria (item 5). Na edição, a inscrição fica como está e a validade é a do campo.
17. O formulário de edição é o único lugar que mostra o CPF inteiro (item 8); abri-lo também vai para o `audit_log` como abertura (`patrons.view`). CPF inválido, data malformada e cada regra do item 4 voltam ao formulário com o que foi digitado e o motivo (422); carteirinha ou CPF repetidos e edição concorrente, com 409.
18. Salvar volta ao formulário de edição com "Salvo". `elzevir staff add --circulation --patrons` concede as duas permissões novas; o primeiro administrador criado pelo `init` já tem as duas.

## 2. Origem no Koha

- Tabelas `categories` e `borrowers` (`reference/koha/installer/data/mysql/kohastructure.sql`).
- `Koha::Patron::Category::get_expiry_date` (meses com `end_of_month => 'limit'`, ou `enrolmentperioddate`) e `Koha::Patron::is_expired`.
- Permissões `circulate` e `borrowers` (`installer/data/mysql/mandatory/userflags.sql`).

## 3. Paridade

`livre` (cadastro e interface), menos a validade, que segue o Koha (item 5). Prova: testes de unidade no domínio (CPF, máscara, validade) e em `elzevir-app::patrons` (limites dos campos); `crates/elzevir-store/tests/patrons.rs` (criar, editar, recusas de carteirinha e CPF repetidos, categoria em uso, edição concorrente, `audit_log`, busca e registro da abertura). `crates/elzevir/tests/patrons.rs`, com o servidor de verdade: permissões de cada página, busca por nome, carteirinha que abre a ficha, CPF mascarado, abertura no `audit_log`, categorias com meses ou data, cadastro com CPF inválido e carteirinha repetida recusados, CPF inteiro só no formulário de edição, edição concorrente recusada.

## 4. Casos de borda

1. Inscrição em 31/01 numa categoria de 1 mês: validade no último dia de fevereiro.
2. CPF `111.111.111-11`: dígitos verificadores batem, mas é recusado.
3. Carteirinha com espaço nas pontas: guardada sem eles; repetida depois do corte é recusada.

## 5. Desvios

- O Koha tem tipo de categoria (adulto, criança, profissional, organização, estatístico), idade mínima e máxima, taxa de inscrição e dezenas de opções por categoria. Elas entram quando uma tarefa precisar delas (limite de débito na 3.4, avisos na 3.9).
- O Koha guarda o histórico de leitura por padrão (`privacy = 1`); o Elzevir só guarda com consentimento.

## 6. LGPD

Os comandos de leitor vão inteiros para o `audit_log`, como todo comando (o log pode ser reaplicado). A anonimização de um leitor, que vem com as ferramentas de LGPD, apaga também os dados dele nesses registros.
