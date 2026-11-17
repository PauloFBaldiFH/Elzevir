# Backup, verificação e restauração — A5/P8

O backup é uma imagem SQLite consistente obtida pela API online a partir de
uma conexão de leitura existente. A conexão gravável do destino pertence
somente ao novo artefato temporário; nenhuma segunda escritora abre o banco
ativo. A cópia inclui o manifesto SHA-256 e versão de esquema, configuração
operacional, configuração local da biblioteca, operações e segredos conhecidos.
O índice é excluído e reconstruído na restauração.

O pacote tar/gzip inteiro é cifrado no formato age, por biblioteca, com uma
chave de recuperação aleatória de 256 bits em arquivo privado `backup.key`.
A chave nunca é incluída no pacote, em argumentos ou logs. O comando de
exportação grava somente um arquivo novo privado. Sem essa chave não é
possível recuperar a cópia; o painel oferece exportação explícita.

A publicação usa arquivo temporário e persistência sem sobrescrita. Diretórios,
links e arquivos preexistentes são recusados quando um destino novo é exigido.
O verificador autentica a cifra, limita entradas e tamanhos, rejeita nomes
arbitrários, links e travessia de diretórios no tar, compara hashes e executa
integrity_check no banco extraído. Esquemas mais novos são recusados.

Uma restauração exige a trava exclusiva da instituição e servidor parado.
Preserva a trava existente enquanto troca o banco e configurações, mantém
cópias de segurança para desfazer erros e reconstrói o índice antes de concluir.
Em uma instalação gerenciada, o cliente para/inicia o serviço e verifica a
saúde, retornando à cópia anterior se a inicialização falhar. Nenhum teste
opera o serviço real ou dados de uma instituição de produção.

Trabalhos do servidor são assíncronos, limitados a uma operação por instituição,
e o IPC aceita identificadores e opções tipadas, sem caminhos arbitrários de
destino. CLI/painel acompanham o mesmo trabalho. Fechar o painel não cancela
uma cópia iniciada. A parada do servidor interrompe trabalhos em pontos seguros.
Retenção remove apenas artefatos regulares reconhecidos e concluídos; mantém
pelo menos a cópia mais nova. Falhas são códigos estáticos sem conteúdo privado.

Origem: A5/P8 da análise do painel de Paulo; liberdade de implementação,
sem regra de paridade com Koha. Dependências novas de arquivo/cifra precisam
passar cargo-deny. Testes usam diretórios temporários e banco fictício.

O certificado de gestão da conta Cloudflare (`cf-cert.pem`) é uma exceção:
pertence ao operador elevado, fica inacessível ao daemon e não entra nos backups
da instituição. A recuperação inclui as credenciais limitadas do conector; para
administrar a conta/DNS depois de recuperar, o operador entra novamente na
Cloudflare. O manifesto recusa esse nome reservado, inclusive variações de caixa.

O preparo de arquivos usa diretório temporário privado do processo, fora do
diretório gravável pelo serviço. Arquivos auxiliares são abertos por capacidade
relativa à instituição: links não podem escapar desse diretório. Uma tabela de
autores externa deve ser movida para a instituição para participar do backup.
O espaço temporário necessário inclui a imagem SQLite descomprimida; o operador
pode selecionar um volume privado maior por `TMPDIR` (ou `TEMP` no Windows).
