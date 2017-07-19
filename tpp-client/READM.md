This dir contains a transport plus protocol sample client that tries to initiate
a connection to the server. The following files are

* checksum.h: the checksum header file 
* stu-cc.out: the binary of the sample client
* tpp_app.h: transport plus protocol application header file
* tpp-client-connect-2stu.c: the main source code of the sample client
* tpp_fsm.h: the finite state machine header file
* tpp.h: the transport plus protocol segment header file 

Note that the sample client assumes there is no packet/segment lost during
transmission. This assumption will not hold in the real test. The main purpose
of the file to make sure some simple communication can be done between a
third-party client and your server by following the transport plus protocol. It
is not for thorough testing purpose. Two main points are the following:

* We will use standard UDP socket API to communicate with studnet's server. This
  mainly to address the question that how a third party can write a client
  without the knowledge of student self-defined transport plus protocol API.
* We expect the application will do the proper byte order conversion before send
  and after receive. Do not assume the two end poins have the same endianness. 

The stu-cc.out was built with _DEBUG_ on, hence it will output debugging
messages to stdout. Note you should not have messages appearing at stdout in
your final submission as per the project description. A sample output from the
client binary is captured below:

---

$ ./stu-cc.out 129.97.56.11 10009
client associated with 129.97.56.11 51231.

p_syn header created:
sport = 51231, dport=10009, sz_seg=20, seq=0, ack=0, flags=0x80, x=0, checksum=0x1033
Sent 20 bytes to 129.97.56.11 10009.

Recvd 20 bytes from 129.97.56.11 10009.
sport = 10009, dport=51231, sz_seg=20, seq=1, ack=0, flags=0xc0, x=0, checksum=0xff2

p_ack header created:
sport = 51231, dport=10009, sz_seg=20, seq=2, ack=0, flags=0x40, x=0, checksum=0x1071
Sent 20 bytes to 129.97.56.11 10009.

Connection successfully established.

