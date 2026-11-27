# Carteirinhas dos leitores

> Status: tarefa 5.6, parte b (carteirinhas). A parte a, as etiquetas de lombada, está em [etiquetas.md](../catalogo/etiquetas.md), com o motor de impressão (Typst embutido). Base: Koha 26.05.05, [doc 03](../../arquitetura/03-estrategia-reconstrucao.md) (tabela da Fase 5, linha 5.6).

## 1. Comportamento

1. `GET /equipe/leitores/carteirinhas?leitor=…` baixa `carteirinhas.pdf` com uma carteirinha para cada leitor dado, na ordem dada, cada um uma vez. Pede a permissão de circulação ou de leitores, como a busca de leitores. São no máximo 50 carteirinhas por vez, o mesmo teto da busca; mais que isso é recusado (413). Sem leitor ou com um valor que não é número, a resposta é 400; se nenhum dos leitores existe (ou todos foram anonimizados), 404. Leitor que não existe no meio dos outros é pulado.
2. **Dados da carteirinha:** nome (prenome e sobrenome), cartão, categoria, unidade de origem, último dia da inscrição (escrito como a língua da interface escreve datas) e o cartão em código de barras. São os dados que a lista da busca já mostra; nada de CPF, contato ou endereço.
3. **Código de barras:** Code 39, o tipo padrão das carteirinhas do Koha, com `*` no começo e no fim. Ele codifica algarismos, letras maiúsculas, espaço e `- . $ / + %`. Um cartão com outro caractere (minúscula, acento) sai sem as barras, só com o número escrito.
4. **Modelo editável:** como as etiquetas, a carteirinha é composta pelo Typst a partir de um modelo `.typ`. O padrão é uma folha A4 com 10 cartões de 85,6 × 54 mm (o tamanho de cartão de crédito), em 2 colunas e 5 linhas, cada um com borda fina para o corte. A unidade vai no alto, depois o nome, a categoria e a validade, e o código de barras com o número embaixo. Para mudar, a biblioteca copia o modelo e o aponta no `elzevir.toml`:

   ```toml
   [printing]
   patron_cards = "modelos/carteirinhas.typ"
   ```

5. O modelo recebe em `sys.inputs.data` a lista `cards` (cada uma com `name`, `surname`, `first_name`, `cardnumber`, `category`, `branch`, `expires` e `bars`, as larguras das barras e espaços em módulos estreitos, começando por barra) e `texts`, as palavras em volta dos dados na língua da interface (`cardnumber` e `expires`). As mesmas regras das etiquetas valem: sem arquivo, sem pacote, sem data, só as fontes do Typst.
6. **Onde imprimir:** a lista da busca de leitores tem o link para imprimir as carteirinhas de todos os encontrados, e a ficha do leitor tem um botão para a carteirinha dele.

## 2. Origem no Koha

- `patroncards/create-pdf.pl` (uma carteirinha por leitor, do lote, da lista ou dos leitores escolhidos) e `C4::Patroncards::Patroncard::_draw_barcode` (Code 39 com `*`).
- O criador de carteirinhas do Koha (`patroncards/`, com modelos, perfis, leiautes e imagens no banco) é substituído pelo modelo Typst (doc 03, §3.4).

## 3. Paridade

`livre` (carteirinhas não estão entre as áreas de paridade exigida). Prova:

- testes de unidade em `elzevir-domain::labels`: barras do Code 39 e recusa do que ele não codifica. A tabela foi conferida com a da biblioteca `python-barcode`, e uma carteirinha impressa foi lida de volta pelo `zbarimg`;
- testes em `elzevir-app::printing`: dados da carteirinha;
- testes em `elzevir-print`: 10 por folha e limite;
- `crates/elzevir/tests/patrons.rs`, contra o servidor: link na busca e botão na ficha, permissão, PDF, leitor repetido, recusas;
- o teste da configuração dos modelos.

## 4. Casos de borda

1. O mesmo leitor duas vezes na lista: uma carteirinha só.
2. Leitor sem prenome: o nome é só o sobrenome.

## 5. Desvios

- O Koha guarda leiautes, imagens e lotes de carteirinhas no banco e permite Code 39 com dígito verificador (`CODE39MOD`, `CODE39MOD10`) e outros tipos; o Elzevir imprime direto da busca ou da ficha, em Code 39 simples, e o resto do desenho fica no modelo Typst.
