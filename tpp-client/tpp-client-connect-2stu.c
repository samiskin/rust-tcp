/*
 * @brief: a sample tpp client program for ECE358S17 course project #2
 * @file: tpp-client-connect-2stu.c 
 * @date 2017/07/17
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>
#include <errno.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include <arpa/inet.h>
#include <netinet/ip.h>
#include <sys/types.h>
#include <ifaddrs.h>
#include <unistd.h>
#include <stdbool.h>
#include <sys/stat.h>
#include <fcntl.h>
#include <signal.h>
#include "net_util.h"
#include "checksum.h"
#include "tpp.h"
#include "tpp_app.h"
#include "tpp_fsm.h"
#include "tpp_var.h"
#include "tpp_subr.h"

#define _DEBUG_	// comment out to remove debugging output on stdout

// global var(s)
struct tppcb g_client_tcb;	// client TCB (Transmission Control Block)

// functions

/**
 * @brief create SYN segment
 * @param p segement hdr, caller allocates memory
 */

void create_SYN_h(struct tpphdr *p) {
	U16 checksum;

	set_seg_seq_h(p, g_client_tcb.iss); 
	set_seg_flags_h(p, TH_SYN);

	checksum = checksum1((const char *)p, p->th_sz_seg);	
	set_seg_checksum_h(p, checksum); 
}

/**
 * @brief create ACK segment
 * @param p segement hdr, caller allocates memory
 */

void create_ACK_h(struct tpphdr *p)
{
	U16 checksum;

	set_seg_ack_h(p, g_client_tcb.rcv_nxt);
	set_seg_flags_h(p, TH_ACK);

	checksum = checksum1((const char *)p, p->th_sz_seg);	
	set_seg_checksum_h(p, checksum); 
}

/**
 * @brief client connection finite state machine
 * @param buf  the received segment
 * @param len  length of the buf
 * @return  0 on success; non-zero otherwise
 * NOTE: really should separate the segment to hdr and payload
 */
int client_fsm(void *buf, size_t len)
{
	size_t sentlen;
	int sockfd = g_client_tcb.t_sock;
	struct tpphdr *p = (struct tpphdr *)buf;
	struct sockaddr_in *p_server = &g_client_tcb.remote;
	struct sockaddr_in *p_client = &g_client_tcb.local;

	switch(g_client_tcb.t_state) {
	case TPPS_SYN_SENT:
		if (p->th_flags != (TH_SYN|TH_ACK)) {
			// error handling to be implemented
			break;
		}

		// It is SYNACK received, update tcb, send ACK
		// update tcb
		g_client_tcb.irs = p->th_seq;
		g_client_tcb.rcv_nxt = p->th_seq + 1;
		// send ACK 
		struct tpphdr *p_ack = ( struct tpphdr *)malloc(sizeof(struct tpphdr)+1);
		init_seg_hdr_h(p_ack, p_client->sin_port, p_server->sin_port);
		create_ACK_h(p_ack);
#ifdef _DEBUG_
		printf("p_ack header created:\n");
		display_tpphdr(p_ack);
#endif // _DEBUG_
		hton_seg(p_ack);
		if((sentlen = sendto(sockfd, (const void *)p_ack, 
			ntohl(p_ack->th_sz_seg) , 0, 
			(const struct sockaddr *)p_server, 
			sizeof(struct sockaddr_in))) < 0) {

			perror("sendto"); 
			return -1;
		}

#ifdef _DEBUG_
		char *q = (char *)(p_ack + 1);
		*q = 0;
		printf("Sent %d bytes to %s %d.\n\n", (int) sentlen, inet_ntoa(p_server->sin_addr), ntohs(p_server->sin_port));
		fflush(stdout);
#endif // _DEBUG_
		// move to ESTABLISHED state
		g_client_tcb.t_state=TPPS_ESTABLISHED;
		break;
	case TPPS_ESTABLISHED:
		break;
	default:
		break;
	}
	return 0;
}


/**
 * @brief: client processes the received segment.
 * @param: void *buf: the segment
 * @param: size_t len: length of the segment (including the header)
 * @return 0 on success; non-zero on failure
 */
int process_segment_client(void *buf, size_t len) {

	// checksum verification 
	if (!verify_checksum1((const char *)buf, (unsigned int) len)) {
		fprintf(stderr, "process_segement_client(): incorrect checksum segment received. \n");
		// extra error handling to be implemented
		return -1;
	}


	return (client_fsm(buf, len));
}

/**
 * @brief client tries to establish a connection to server
 * @param sockfd: the client socket file descriptor 
 * @param addr: addr of the server to connect to
 * @param addrlen: server addr length
 * @return 0 on success and non-zero on failure
 * NOTE: No packet loss assumed 
 * TODO: errno setting is not done yet
 */

int tpp_connect()
{
	ssize_t sentlen;
	int sockfd = g_client_tcb.t_sock;
	struct sockaddr_in *p_server = &g_client_tcb.remote;
	struct sockaddr_in *p_client = &g_client_tcb.local;

	// client send SYN
	struct tpphdr *p_syn = (struct tpphdr *)malloc(sizeof(struct tpphdr)+1);
	init_seg_hdr_h(p_syn, p_client->sin_port, p_server->sin_port);
	create_SYN_h(p_syn);
#ifdef _DEBUG_
	printf("p_syn header created:\n");
	display_tpphdr(p_syn);
#endif // _DEBUG_
	hton_seg(p_syn); // convert to network byte order before send
	if((sentlen = sendto(sockfd, (const void *)p_syn, 
		ntohl(p_syn->th_sz_seg) , 0, (const struct sockaddr *)p_server,
		sizeof(struct sockaddr_in))) < 0) {

		perror("sendto"); 
		return -1;
	}
#ifdef _DEBUG_
	char *q = (char *)(p_syn+1);
	*q = 0;
	printf("Sent %d bytes to %s %d.\n\n", (int) sentlen, inet_ntoa(p_server->sin_addr), ntohs(p_server->sin_port));
#endif // _DEBUG_

	// Transfer to SYN_SENT state
	g_client_tcb.t_state = TPPS_SYN_SENT;

	// to receiv SYN+ACK from the server
	size_t buflen = MAX_BUF_LEN;
	char buf[buflen];
	ssize_t recvlen;
	socklen_t alen = sizeof(struct sockaddr_in);

	if((recvlen = recvfrom(sockfd, buf, buflen-1, 0, 
		(struct sockaddr *)p_server, &alen)) < 0) { 

		perror("recvfrom");
		//TODO: error handling  
	        return -1;
	}
#ifdef _DEBUG_
	buf[recvlen] = '\0'; // ensure null-terminated string
	printf("Recvd %d bytes from %s %d.\n",
		(int)recvlen, inet_ntoa(p_server->sin_addr), ntohs(p_server->sin_port));
	fflush(stdout);
#endif // _DEBUG_
	ntoh_seg(buf);	// convert to host byte order after receive

#ifdef _DEBUG_
	display_tpphdr((struct tpphdr *)buf);
	printf("\n");
#endif // _DEBUG_

	return (process_segment_client(buf, recvlen));
}

/**
 * @brief generate a random number
 * NOTE: note used in the sample to generate the initial sequence number yet
 * leave it here for students as a reference
 */
unsigned int getrand() {
	int f = open("/dev/urandom", O_RDONLY);
	if(f < 0) {
		perror("open(/dev/urandom)"); return 0;
	}

	unsigned int ret;
	read(f, &ret, sizeof(unsigned int));
	close(f);
	return ret;
}

int main(int argc, char *argv[]) {
	if(argc != 3) {
		printf("Usage: %s server-ip server-port\n.", argv[0]);
		return -1;
	}

	// initialize client TCB data structure
	init_tppcb(&g_client_tcb);
	g_client_tcb.t_id = 1;

	int sockfd = -1;
	if((sockfd = socket(AF_INET, SOCK_DGRAM, 0)) < 0) {
		perror("socket"); 
		return -1;
	}

	g_client_tcb.t_sock = sockfd;

	struct sockaddr_in *p_server = &(g_client_tcb.remote);
	bzero(p_server, sizeof(struct sockaddr_in));
	p_server->sin_family = AF_INET;
	if(!inet_aton(argv[1], &(p_server->sin_addr))) {
		perror("invalid server-ip"); 
		exit(1);
	}
	p_server->sin_port = htons(atoi(argv[2]));


	struct sockaddr_in *p_client = &(g_client_tcb.local);
	bzero(p_client, sizeof(struct sockaddr_in));
	p_client->sin_family = AF_INET;
	p_client->sin_port = 0; // Let OS choose.
	if ((p_client->sin_addr.s_addr = getPublicIPAddr()) == 0) {
		fprintf(stderr, "Unable to get public ip address. Exiting...\n");
		exit(1);
	}

	if(bind(sockfd, (struct sockaddr *)p_client, sizeof(struct sockaddr_in)) < 0) {
		perror("bind"); 
		exit(1);
	}

	socklen_t alen = sizeof(struct sockaddr_in);
	if(getsockname(sockfd, (struct sockaddr *)p_client, &alen) < 0) {
		perror("getsockname"); 
		exit(1);
	}
#ifdef _DEBUG_
	printf("client associated with %s %d.\n\n",
		inet_ntoa(p_client->sin_addr), ntohs(p_client->sin_port));
#endif //_DEBUG_

	// initiate a connection 
	if ( tpp_connect() == 0) {
#ifdef _DEBUG_
		printf("Connection successfully established.\n");
#endif // _DEBUG_
		//TODO start receive/send data here
	} else {
		fprintf(stderr, "tpp_connect() failed.\n");
	}

	if(close(sockfd) < 0) {
		perror("close"); return -1;
	}

	return 0;
}
