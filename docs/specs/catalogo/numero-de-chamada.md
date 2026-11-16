# Número de chamada sugerido (v0)

> Status: v0, tarefa 2.16. Base: [exemplares](exemplares.md), [bibframe-completo](bibframe-completo.md), item 10 (o Cutter é a parte do item do número de chamada, não classe da ontologia), e doc 04 (Palotina: CDD e tabela PHA). Critério da tarefa: **o campo sugere o Cutter do autor pela tabela configurada; o catalogador confirma ou troca**.

## 1. Comportamento

### A tabela

1. A tabela de autores (Tabela PHA, de Heloísa de Almeida Prado, ou uma Cutter-Sanborn) tem direitos autorais e **não acompanha o Elzevir**. Cada biblioteca entrega o próprio exemplar como arquivo de texto e o indica no `elzevir.toml` do diretório de dados:

   ```toml
   [call_number]
   author_table = "tabelas/pha.txt"   # relativo ao diretório de dados
   ```

   Sem a chave, não há sugestão. Uma chave desconhecida, um arquivo que falta ou uma tabela sem entradas impedem o `serve` de iniciar, com a mensagem do erro.
2. Cada linha é uma linha do livro: `entrada número` (`Assi 848`), `número entrada` (`  848     Assi`, como a Cutter-Sanborn imprime) ou a linha da PHA, `entrada número entrada` (`Lent 589 Leo`). Tabulação ou `;` podem separar as partes; a vírgula só quando está junto do número (`Sampaio M.,184`). Linhas vazias e começadas por `#` não contam; um BOM e fins de linha do Windows são aceitos. Limites: 8 MiB, 100 000 linhas, 512 bytes por linha.
3. Ao iniciar, o servidor diz quantas entradas leu e lista, para conferir no livro, as linhas que não entendeu, os números fora de ordem (a numeração é decimal: `1, 11, 111, 112, 12` comparam como texto) e as entradas repetidas (fica a primeira).

### A notação de autor

4. A regra é a da explicação da tabela PHA, a mesma da Calculadora Cutter do Koha Nexus: a entrada **imediatamente anterior** ao nome em ordem alfabética (um nome antes de todas as entradas da sua letra fica com a primeira), a inicial do sobrenome em maiúscula, o número e a inicial do título.
5. A comparação usa o nome em minúsculas, sem acentos, só com letras, algarismos e espaços simples; `M'` e `Mc` são lidos como `Mac`; os prefixos `van der`, `van den`, `van de`, `de la`, `de las`, `de los`, `von der`, `von`, e `la`, `le`, `les`, `du`, `des`, `van`, `ten`, `ter`, `del`, `della`, `delle`, `di`, `da`, `de`, `dos`, `das`, `do`, `lo`, `el`, `al` juntam-se à palavra seguinte (`La Fontaine` → `lafontaine`); o apóstrofo some (`O'Donnel` → `odonnel`).
6. A entrada principal decide: pessoa ou família (`Sobrenome, Prenome`), pelo nome; organização, evento ou jurisdição, pela primeira palavra; sem entrada principal, pela primeira palavra do título sem o artigo, e sem a inicial do título.
7. A inicial do título vem do título próprio da Instância (antes de ` : ` e ` / `), sem o artigo inicial (`o`, `a`, `os`, `as`, `um`, `uma`, `uns`, `umas`, `the`, `an`, `el`, `la`, `los`, `las`, `lo`, `le`, `les`, `un`, `une`, `il`, `gli`, `der`, `die`, `das`, `ein`, `eine`, e `l'`); um título em `l` leva `L` maiúsculo, para não se confundir com o algarismo 1.

### A sugestão

8. O número de chamada sugerido é a classificação principal da Obra (a CDD em Palotina), a notação de autor e, a partir da 2ª edição, `N. ed.` (lido do começo da menção de edição): `Assis, Machado de` + *Dom Casmurro*, 2. ed., em 869.3 → `869.3 A848d 2. ed.`.
9. A grade de exemplares mostra a sugestão acima das linhas, com a entrada da tabela de onde veio a notação, e uma linha nova cuja coluna de número de chamada ficaria vazia começa com ela, em itálico, até ser salva ou trocada. Linhas novas depois de uma linha preenchida continuam copiando o número de chamada da linha de cima ([exemplares](exemplares.md), item 19).
10. Sem tabela, ou quando a tabela não tem entrada para a letra do nome (ou o nome não começa por letra), não há sugestão, e a grade fica como antes.

## 2. Origem no Koha

- O Koha não tem tabela de autores: `cataloguing/value_builder/callnumber.pl` monta o número de chamada a partir da classificação. A notação vem da Calculadora Cutter do Koha Nexus (`KohaEasy::Cataloguing::Rules::notation`), que segue a explicação da tabela PHA.

## 3. Paridade

`livre`: interface e configuração. As opções da Calculadora Cutter do Koha Nexus (sem a letra do título, exemplar, entrada principal fixada, ler a tabela de um PDF) ficam para depois.

Testes: `crates/elzevir-domain/src/call_number.rs` (tabela e notação), `crates/elzevir-app/src/call_number.rs` (sugestão), `crates/elzevir/src/config.rs` (`elzevir.toml`) e `crates/elzevir-web/tests/staff_catalog.rs` (grade).
