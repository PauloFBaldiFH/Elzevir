# Operações interativas — P8/P10/P11/P12

Uma ação de escrita nunca se mistura à coalescência de atualizações de leitura.
A UI aceita no máximo uma solicitação explícita pendente. Formulários têm
campos limitados e confirmação separada; Esc cancela antes da submissão e a
repetição de tecla não duplica o comando. A confirmação identifica a operação
e a instituição selecionada. Abrir uma seção não executa ações.

Backups: criar agora, verificar o último concluído, exportar arquivo cifrado,
exportar chave de recuperação para destino novo, restaurar arquivo/chave após
confirmação e servidor parado, e alterar retenção/agendamento diário com fuso.
A tela apresenta resultados reais do trabalhador, inclusive falhas e
interrupções. Não mostra conteúdo de arquivos, chaves ou credenciais em logs.
Atualizações do painel continuam durante trabalhos longos do servidor.

Reinício e atualização exigem confirmação; a indisponibilidade de observação
de sessões nunca é apresentada como zero sessões. A operação de sistema é
executada por cliente privilegiado, fora do processo que será parado.

A contagem de sessões é calculada no próprio estado em memória do servidor,
expurgando as expiradas sem renovar seu prazo. A resposta administrativa só
contém o número, nunca identificadores, logins ou cookies. A contagem exibida é
uma observação, não um bloqueio transacional contra novos acessos.

As operações locais demoradas que exigem parada rodam em processo auxiliar do
mesmo executável, com entrada privada, limites de leitura e recibo de resultado.
Fechar o painel não mata uma restauração em andamento. O auxiliar serializa
operações de host com trava própria e não aceita pedidos do IPC do daemon.
Nenhum token ou conteúdo de chave é passado em argumentos de subprocessos.

Atualização: fixar a versão e verificar o pacote antes de parar o serviço;
efetuar e verificar backup cifrado; só então parar, substituir executável,
iniciar e consultar saúde. Se iniciar/saúde falhar, parar a versão nova,
restaurar executável e dados anteriores (a versão nova pode ter migrado o
esquema), iniciar a anterior e verificar novamente. Falha de download, hash ou
backup nunca para o serviço. No Windows o auxiliar é uma cópia do executável,
permitindo substituir o arquivo original enquanto o painel ainda está aberto.
## Recuperação de atualização

Na seção Atualizações, `r` prepara a recuperação de uma atualização interrompida.
O operador informa o executável instalado (vazio usa o atual) e confirma a ação.
A composição valida que o diário privado pertence à configuração e instituição
selecionadas antes de aceitar o trabalho. O painel não interpreta o diário nem
executa comandos de serviço diretamente.


Recibos de operações de host são limitados a 256. A retenção remove somente
recibos reconhecidos de trabalhos encerrados, preservando trabalhos ativos e
arquivos desconhecidos. Uma trava por trabalho permite reconhecer processo
interrompido sem confundir reutilização de PID. Solicitação que não chega a
iniciar em cinco minutos aparece como interrompida. A observação não escreve
no histórico; a limpeza acontece na publicação de um novo recibo.

Tickets e diretórios temporários de operações são privados desde a criação,
independentemente da umask do processo. Em Unix, o criador solicita modo 0700
para o diretório e 0600 para arquivos privados. A validação continua recusando
tickets que o operador ou outro processo tornou acessíveis a grupo/outros.
O mesmo criador privado é usado para estagiar dados de backup e restauração.
