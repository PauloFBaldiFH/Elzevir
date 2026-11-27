# Etiquetas de lombada

> Status: tarefa 5.6, parte a (etiquetas de lombada). A parte b, as carteirinhas dos leitores, está em [carteirinhas.md](../circulacao/carteirinhas.md). Base: Koha 26.05.05, [doc 03](../../arquitetura/03-estrategia-reconstrucao.md) (tabela da Fase 5, linha 5.6) e [doc 02](../../arquitetura/02-modo-de-implementacao.md) (Typst para documentos impressos).

## 1. Comportamento

1. Na barra da seleção do acervo, **Etiquetas de lombada (PDF)** baixa `etiquetas.pdf` com uma etiqueta para cada exemplar das famílias selecionadas, família por família na ordem da seleção e, dentro dela, na ordem da listagem. Cada família entra uma vez. Pede sessão da equipe, como a exportação MARC da seleção.
2. Entra o exemplar com número de chamada. O que não tem número (ou só tem espaços) fica de fora, e o cabeçalho `x-elzevir-without-call-number` da resposta diz quantos foram. Se nenhum exemplar da seleção tem número, a resposta é 400, sem PDF.
3. Uma impressão tem no máximo 5.000 etiquetas; mais que isso é recusado (413).
4. **Linhas da lombada.** O número de chamada é dividido como no Koha para a CDD: sem as barras, a classe (com as letras que vêm antes dela, como `R` ou `CD-ROM`, numa linha própria) e depois cada palavra do resto numa linha. `869.3 A474d v.2 ex.3` dá `869.3`, `A474d`, `v.2`, `ex.3`; `R220.3 H2793Z H32 c.2` dá `R`, `220.3`, `H2793Z`, `H32`, `c.2`. O que não começa por uma classe de três algarismos ou mais é dividido nos espaços.
5. **Modelo editável.** O PDF é composto pelo Typst, embutido no servidor, a partir de um modelo `.typ`. O modelo padrão é uma folha A4 de 65 etiquetas de 38,1 × 21,2 mm (5 colunas e 13 linhas), com as linhas centradas em DejaVu Sans Mono negrito de 9 pt, e as medidas da folha ficam no topo do arquivo. Para outra folha, a biblioteca copia o modelo para a pasta de dados, ajusta as medidas e o aponta no `elzevir.toml`:

   ```toml
   [printing]
   spine_labels = "modelos/etiquetas.typ"
   ```

   O caminho é relativo à pasta de dados. Arquivo ausente ou maior que 1 MiB impede o servidor de iniciar, com a mensagem.
6. O modelo recebe os dados em JSON em `sys.inputs.data`: `labels`, a lista das etiquetas, cada uma com `call_number`, `lines`, `barcode`, `title` (da Instância) e `author` (o contribuidor principal da Obra); os dois últimos e o código podem ser `none`. Assim um modelo pode pôr título, autor ou código na etiqueta.
7. O modelo não lê arquivo nenhum, não baixa pacote e não vê a data; só tem as fontes embutidas no Typst (Libertinus Serif, New Computer Modern e DejaVu Sans Mono). Um erro no modelo responde 500 com as mensagens do Typst.

## 2. Origem no Koha

- `C4::Labels::Label::draw_label_text` (uma etiqueta por exemplar, número de chamada dividido quando `callnum_split` está ligado) e `C4::ClassSplitRoutine::Dewey::split_callnumber`, com `t/Labels_split_ddcn.t`.
- O criador de etiquetas do Koha (`labels/`: modelos, perfis e lotes com coordenadas em PDF) é substituído pelo modelo Typst (doc 03, §3.4).

## 3. Paridade

`livre` (etiquetas não estão entre as áreas de paridade exigida), com a divisão do número de chamada igual à do Koha. Prova: testes de unidade em `elzevir-domain::labels` (os casos de `t/Labels_split_ddcn.t`, conferidos também rodando a sub do Koha), em `elzevir-app::printing` (uma etiqueta por exemplar com número) e em `elzevir-print` (PDF com uma página por folha, modelo com erro, modelo que tenta ler arquivo, limite); `crates/elzevir-web/tests/staff_catalog.rs` (link na barra, sessão, PDF, cabeçalho, ordem e repetição da seleção, sem impressora); `crates/elzevir/tests/batch.rs` (etiquetas das famílias do acervo de exemplo compostas pelo Typst); e o teste da configuração do modelo.

## 4. Casos de borda

1. Dois espaços depois das letras (`REF  x`): o primeiro fica com as letras, como no Koha.
2. Barra no meio (`869.3/A474d`): sai, e o número fica numa linha só (`869.3A474d`), como no Koha.
3. Número com espaços em volta: aparado antes da divisão.

## 5. Desvios

- O Koha guarda modelos, perfis de impressora e lotes de etiquetas no banco e monta o PDF por coordenadas; o Elzevir imprime direto da seleção do acervo, e folha, margens e conteúdo ficam no modelo Typst.
- O Koha oferece outras divisões (LCC, NLM, expressão regular) e tipos de etiqueta com código de barras; aqui a divisão é a da CDD, que é a da biblioteca; o código de barras do exemplar vai na etiqueta quando o modelo o escreve.
- O Koha não apara o número de chamada antes de dividir; o Elzevir apara.
