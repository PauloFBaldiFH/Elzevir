# Prévia de relações entre fontes (1c)

O operador confere como os registros de uma instalação se referem uns aos
outros antes de propor qualquer escrita. O relatório é privado e vinculado ao
lote, à revisão, ao digest do manifesto e aos digests dos membros. Uma posição
de registro indica membro, linha a partir de 1 e deslocamento em bytes no
membro. As referências são reproduzíveis a partir do original.

Uma receita v2 nomeia os conjuntos, leitores, componentes ordenados
da identidade e relações direcionadas. Cada relação declara conjunto filho,
chave estrangeira, conjunto pai e cardinalidade esperada (vários para um ou
um para um). Uma tabela de associação pode ter chave composta e duas relações.
As relações item → holding e holding → instância preservam o nível intermediário.
Cada conjunto declara `source_sha256` com o digest do membro no inventário
ou `null` quando o papel está ausente. O avaliador exige uma única ocorrência
do digest no lote. Renomear ou reordenar os arquivos conserva a receita e os
vínculos. Conteúdo alterado exige revisar o novo inventário e atualizar o
digest do papel correspondente. As chaves e relações são reutilizadas.
A posição efetiva aparece em `report.identities[].member`, na ordem dos
conjuntos. Uma receita v1 da prévia experimental usa posições relativas:
para migrar, conferir cada membro no inventário original, trocar `member`
por `source_sha256` e usar a versão 2. Atribuições apenas posicionais são
recusadas, inclusive quando os arquivos têm a mesma estrutura.

A leitura aceita um objeto JSON por linha ou um envelope TSV com ID textual
na primeira coluna e objeto JSON na segunda. Campos adicionais ficam no
original. Caminhos seguem JSON Pointer. Identificadores textuais conservam
zeros, espaços significativos e tipo. Componentes inteiros têm tipo próprio.
O operador pode exigir que o ID do envelope concorde com a identidade JSON.
Um conflito impede usar ambas as formas como prova de identidade.

A prévia distingue vínculo único, ausência de pai, referência órfã, ambiguidade,
identidade conflitante, violação de cardinalidade e chave indisponível.
Distingue componentes ausentes, nulos, vazios e inválidos. Duplicatas apontam
para as linhas candidatas. A cardinalidade um para um considera todas as
linhas antes de declarar um vínculo único. A contabilidade e os exemplos
localizados cobrem também problemas fora da página solicitada.

O perfil opcional `folio-v1` propõe uma receita pela estrutura observada.
Arquivos renomeados são elegíveis. Atribuições concorrentes exigem revisão.
Uma receita explícita usa o mesmo avaliador de relações. A evidência inclui
o plano executado e o método de seleção. A avaliação abrange os originais
do lote privado e conserva o catálogo BIBFRAME para a etapa de projeção.

Critérios de aceitação: Cornell com 62/107/536 linhas e 107+536 referências
resolvidas; 5kItems com 47.745/49.580/0 e 49.580 referências resolvidas;
esquema independente com chaves compostas, hierarquia, órfãos, ambiguidade e
conflito; troca de revisão ou de bytes, entrada truncada e ausência de permissão
recusadas. Medir tempo e memória nas duas exportações.

## Operação e leitura da evidência

No Staff, abrir o conjunto privado e usar **Conferir relações**. Selecionar
FOLIO v1 ou colar uma receita JSON explícita. A primeira linha da página é
configurável. A CLI aceita os dois modos sobre o mesmo lote.

```sh
elzevir import relations --data-dir DIR --operator 2 LOTE 1 --profile folio-v1 --from 100
elzevir import relations --data-dir DIR --operator 2 LOTE 1 --plan receita.json
```

Exemplo de receita para uma exportação independente. A ordem dos componentes
faz parte da chave. O JSON do relatório inclui a receita efetivamente usada
em `report.plan`, que pode ser salva e revista pela opção `--plan`. Os dois originais abaixo têm um objeto por linha em UTF-8, com LF final.

```jsonl
{"filial":"S","codigo":"01"}
```

```jsonl
{"id":"C1","local":{"filial":"S","setor":"01"}}
```

```json
{
  "version": 2,
  "datasets": [
    {"name":"setores","source_sha256":"d29ae0a1d0d77655f0bfd8839ad17fc9c08c86fc7ee12104022c24c8511ac647","encoding":"json_lines","key":["/filial","/codigo"]},
    {"name":"caixas","source_sha256":"57dcf52b60698a7f60c28b99044854c697171a563a2a688ba2e32c5b018ae7ef","encoding":"json_lines","key":["/id"]}
  ],
  "relations": [
    {"child":"caixas","parent":"setores","foreign":["/local/filial","/local/setor"],"cardinality":"many_to_one"}
  ]
}
```

A identificação do registro exige uma chave completa. Uma chave filha
duplicada impede atestar seu vínculo. Na verificação um para um, todas
as referências completas contam como reivindicações, inclusive quando
outros dados daquela linha precisam de correção. Uma etapa resolvida
pode terminar em ancestral órfão. A página distingue esses dois resultados
e identifica a regra e a linha do ancestral. Interrompe o percurso em
ramificações, ciclos ou após 16 passos adicionais. Em uma tabela de associação,
cada papel permanece disponível por sua própria relação. As regras no JSON
são numeradas a partir de zero. Membros e linhas começam em 1.

O perfil percorre todos os registros em busca de combinações de propriedades
tipadas. Por exemplo, `instanceTypeId` e `title` fornecem evidência de
Instância. O relatório mostra propriedades observadas e frequência por
membro. Um membro bem formado sem assinatura reconhecida fica em
`unassigned` quando os demais permitem reconhecer o conjunto. Dois candidatos
para o mesmo papel ou sinais contraditórios no mesmo membro pedem receita
explícita. Para um lote com formatos heterogêneos, a receita pode selecionar
somente os membros pertinentes. A releitura integral verifica também os
digests das fontes omitidas. Linhas ou propriedades JSON malformadas ou
duplicadas recusam a análise sem ecoar seus valores.

O leitor trabalha por linha (até 256 KiB, 200 mil linhas por passagem).
O índice contém chaves tipadas de até 4 KiB, posições e contagens. Os limites
são 64 conjuntos, 64 relações, oito componentes por chave, 400 mil entradas
de índice, 48 MiB cumulativos de bytes de chaves e um milhão de avaliações
de relação por passagem. Um limite atingido devolve uma recusa integral.
A prévia exibe no máximo 20 linhas por relação, os totais globais e um
exemplo por resultado. Uma análise de relações por instituição pode executar
a cada vez. O operador pode cancelar o lote durante a leitura. A presença
do original é consultada a cada MiB, e o manifesto é revalidado ao terminar.
O relatório é recalculado a pedido e usa somente o staging privado da 1b.
