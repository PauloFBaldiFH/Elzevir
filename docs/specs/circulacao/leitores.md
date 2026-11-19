# Leitores e categorias de leitor

> Status: tarefa 3.1, parte a (modelo e comandos). A parte b (busca, ficha e telas) vem no PR seguinte. Base: Koha 26.05.05, [doc 03](../../arquitetura/03-estrategia-reconstrucao.md) (tabela da Fase 3, linha 3.1) e [interface](../../UI_BIBFRAME_INTERACTION_SPEC.md) §18.4.

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

## 2. Origem no Koha

- Tabelas `categories` e `borrowers` (`reference/koha/installer/data/mysql/kohastructure.sql`).
- `Koha::Patron::Category::get_expiry_date` (meses com `end_of_month => 'limit'`, ou `enrolmentperioddate`) e `Koha::Patron::is_expired`.
- Permissões `circulate` e `borrowers` (`installer/data/mysql/mandatory/userflags.sql`).

## 3. Paridade

`livre` (cadastro e interface), menos a validade, que segue o Koha (item 5). Prova: testes de unidade no domínio (CPF, máscara, validade) e em `elzevir-app::patrons` (limites dos campos); `crates/elzevir-store/tests/patrons.rs` (criar, editar, recusas de carteirinha e CPF repetidos, categoria em uso, edição concorrente, `audit_log`).

## 4. Casos de borda

1. Inscrição em 31/01 numa categoria de 1 mês: validade no último dia de fevereiro.
2. CPF `111.111.111-11`: dígitos verificadores batem, mas é recusado.
3. Carteirinha com espaço nas pontas: guardada sem eles; repetida depois do corte é recusada.

## 5. Desvios

- O Koha tem tipo de categoria (adulto, criança, profissional, organização, estatístico), idade mínima e máxima, taxa de inscrição e dezenas de opções por categoria. Elas entram quando uma tarefa precisar delas (limite de débito na 3.4, avisos na 3.9).
- O Koha guarda o histórico de leitura por padrão (`privacy = 1`); o Elzevir só guarda com consentimento.

## 6. LGPD

Os comandos de leitor vão inteiros para o `audit_log`, como todo comando (o log pode ser reaplicado). A anonimização de um leitor, que vem com as ferramentas de LGPD, apaga também os dados dele nesses registros.
